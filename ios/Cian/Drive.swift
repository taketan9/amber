import Foundation
import AuthenticationServices
import CryptoKit
import Security
import UIKit

/// **Google Drive と話す口**（電話・依頼 500）── 窓の `gui/drive.js` の写し。
///
/// 押すのは「Google でサインイン」だけ。URL は打たせない・Drive の設定画面は
/// 触らせない（本人が決めた・案 甲）。鍵はキーチェーンに。**判断はしない** ──
/// 何を運ぶかは core の `syncplan`、運ぶのは `Syncing`。ここは通信だけ。
///
/// iOS 用のクライアント ID には秘密が無い（Google がそう決めている）ので、
/// 焼き込んでよい。折り返し先はそのクライアントの URL スキーム。
final class Drive {
    static let shared = Drive()

    static let clientId = "306373349806-paqosepmbnclbibibnq9ktodk42h6qbr.apps.googleusercontent.com"
    static let scheme = "com.googleusercontent.apps.306373349806-paqosepmbnclbibibnq9ktodk42h6qbr"
    static let redirect = scheme + ":/oauth2redirect"
    static let scope = "https://www.googleapis.com/auth/drive.file"
    /// **カレンダーの許可は、要る瞬間に足す**（依頼 532・窓の `CAL_SCOPE` と同じ）。
    /// ノートの同期しか使わない人に、カレンダーの許可を訊かない ── 同意の画面に
    /// 並ぶ数が増えるほど、押す前に引き返す人が増える。`calendar.app.created` は
    /// 「アプリが自分で作った二次カレンダーだけ」で、**非機密**（審査が要らない）。
    static let calScope = "https://www.googleapis.com/auth/calendar.app.created"
    static let homeName = "ambər"

    /// 試験のための差し替え（`walk-phone.sh` が偽の Drive を指す）。人の道には出ない。
    private let env = ProcessInfo.processInfo.environment
    private var apiUrl: String { env["AMBER_DRIVE_URL"] ?? "https://www.googleapis.com" }
    private var tokenUrl: String { env["AMBER_DRIVE_URL"].map { $0 + "/token" } ?? "https://oauth2.googleapis.com/token" }
    private var aboutUrl: String { env["AMBER_DRIVE_URL"].map { $0 + "/about" } ?? "https://www.googleapis.com/drive/v3/about?fields=user" }
    private var revokeUrl: String { env["AMBER_DRIVE_URL"].map { $0 + "/revoke" } ?? "https://oauth2.googleapis.com/revoke" }
    private var fakeToken: String? { env["AMBER_DRIVE_TOKEN"] }

    /// この端末の名前（向こうの端末に「誰の版か」と見せる札）。
    var by: String { env["AMBER_DEVICE"] ?? UIDevice.current.name }

    struct Who: Codable, Equatable { let name: String; let email: String }
    struct Kept: Codable {
        var access: String
        var refresh: String?
        var until: TimeInterval
        var who: Who?
        /// **貰えた許可**（頼んだ許可ではない）── 断られたものを持っていると
        /// 思い込むと、使う瞬間まで気づけない。
        var scope: String?
    }
    struct Remote { let rel: String; let id: String; let tag: String; let by: String }

    enum Trouble: LocalizedError {
        case signedOut, http(Int, String), bad(String), cancelled
        var errorDescription: String? {
            switch self {
            case .signedOut: return "同期のサインインが切れています。設定の「同期」からもう一度サインインしてください"
            case .http(let code, let why): return why.isEmpty ? "HTTP \(code)" : why
            case .bad(let why): return why
            case .cancelled: return "サインインをやめました"
            }
        }
    }

    // ── 鍵 ──

    private static let keychainKey = "amber.drive.token"

    private func load() -> Kept? {
        if let fake = fakeToken {
            return Kept(access: fake, refresh: nil, until: Date().timeIntervalSince1970 + 3600,
                        who: Who(name: "試し", email: "test@example.com"),
                        scope: Self.scope + " " + Self.calScope)
        }
        guard let data = Keychain.get(Self.keychainKey) else { return nil }
        return try? JSONDecoder().decode(Kept.self, from: data)
    }
    private func store(_ kept: Kept) {
        if let data = try? JSONEncoder().encode(kept) { Keychain.set(Self.keychainKey, data) }
    }
    private func forget() { Keychain.delete(Self.keychainKey) }

    var signedIn: Bool { load() != nil }
    var who: Who? { load()?.who }

    // ── サインイン（OAuth 2.0・PKCE）──

    private static func b64url(_ d: Data) -> String {
        d.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    /// その許可を持っているか。**使う前に訊く** ── 持っていないまま叩くと、
    /// 人には「HTTP 403」としか見えない。
    func grants(_ one: String) -> Bool {
        guard let kept = load() else { return false }
        return (kept.scope ?? Self.scope).split(separator: " ").map(String.init).contains(one)
    }

    /// ブラウザで「許可」を押してもらい、鍵に換えてキーチェーンへ。返すのは誰か。
    /// `want` を渡すと、その許可だけを足しにいく。
    ///
    /// **`@MainActor` はここに付いていないといけない** ── ブラウザの画面を
    /// 出すので。一度、この上に別の関数を挟んで付け替えてしまい、**それでも
    /// ビルドは通った**（2026-09-13）。
    @MainActor
    func signIn(want: String? = nil) async throws -> Who {
        var raw = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, raw.count, &raw)
        let verifier = Self.b64url(Data(raw))
        let challenge = Self.b64url(Data(SHA256.hash(data: Data(verifier.utf8))))
        var state = [UInt8](repeating: 0, count: 16)
        _ = SecRandomCopyBytes(kSecRandomDefault, state.count, &state)
        let stateWord = Self.b64url(Data(state))
        var parts = URLComponents(string: "https://accounts.google.com/o/oauth2/v2/auth")!
        parts.queryItems = [
            .init(name: "client_id", value: Self.clientId),
            .init(name: "redirect_uri", value: Self.redirect),
            .init(name: "response_type", value: "code"),
            .init(name: "scope", value: want ?? Self.scope),
            // **前に貰った許可を落とさない。** これが無いと、カレンダーの許可を
            // 足しにいった瞬間に Drive の許可が消えて、同期が黙って止まる。
            .init(name: "include_granted_scopes", value: "true"),
            .init(name: "code_challenge", value: challenge),
            .init(name: "code_challenge_method", value: "S256"),
            .init(name: "state", value: stateWord),
            .init(name: "access_type", value: "offline"),
            .init(name: "prompt", value: "consent"),
        ]
        let callback: URL = try await withCheckedThrowingContinuation { go in
            let session = ASWebAuthenticationSession(url: parts.url!, callbackURLScheme: Self.scheme) { url, err in
                if let url { go.resume(returning: url) }
                else if let e = err as? ASWebAuthenticationSessionError, e.code == .canceledLogin { go.resume(throwing: Trouble.cancelled) }
                else { go.resume(throwing: Trouble.bad(err?.localizedDescription ?? "ブラウザから戻ってきませんでした")) }
            }
            session.presentationContextProvider = Presenter.shared
            session.prefersEphemeralWebBrowserSession = false
            if !session.start() { go.resume(throwing: Trouble.bad("ブラウザを開けませんでした")) }
        }
        let q = URLComponents(url: callback, resolvingAgainstBaseURL: false)?.queryItems ?? []
        let got = Dictionary(uniqueKeysWithValues: q.map { ($0.name, $0.value ?? "") })
        if let e = got["error"] { throw Trouble.bad("Google に断られました: " + e) }
        guard got["state"] == stateWord else { throw Trouble.bad("合言葉が違います（別のサインインの返事）") }
        guard let code = got["code"], !code.isEmpty else { throw Trouble.bad("許可の符号がありません") }
        let tok = try await post(tokenUrl, form: [
            "client_id": Self.clientId, "code": code, "code_verifier": verifier,
            "redirect_uri": Self.redirect, "grant_type": "authorization_code",
        ])
        guard let access = tok["access_token"] as? String else { throw Trouble.bad("鍵が返ってきませんでした") }
        var kept = Kept(access: access, refresh: tok["refresh_token"] as? String,
                        until: Date().timeIntervalSince1970 + ((tok["expires_in"] as? Double) ?? 3600) - 60,
                        who: nil, scope: (tok["scope"] as? String) ?? want ?? Self.scope)
        kept.who = try? await whoAmI(access)
        store(kept)
        return kept.who ?? Who(name: "", email: "")
    }

    /// やめる ── Google 側の許可も取り消して、鍵を捨てる。
    func signOut() async {
        let kept = load()
        forget()
        if let kept, fakeToken == nil {
            var req = URLRequest(url: URL(string: revokeUrl + "?token=" + (kept.refresh ?? kept.access))!)
            req.httpMethod = "POST"
            _ = try? await URLSession.shared.data(for: req)
        }
    }

    private func whoAmI(_ access: String) async throws -> Who {
        var req = URLRequest(url: URL(string: aboutUrl)!)
        req.setValue("Bearer " + access, forHTTPHeaderField: "Authorization")
        let (data, _) = try await URLSession.shared.data(for: req)
        let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        let user = obj?["user"] as? [String: Any]
        return Who(name: user?["displayName"] as? String ?? "", email: user?["emailAddress"] as? String ?? "")
    }

    /// いま使える鍵。切れていれば黙って新しくする。無ければ nil。
    private func token() async throws -> String? {
        guard var kept = load() else { return nil }
        if Date().timeIntervalSince1970 < kept.until { return kept.access }
        guard let refresh = kept.refresh else { return nil }
        let tok = try await post(tokenUrl, form: [
            "client_id": Self.clientId, "refresh_token": refresh, "grant_type": "refresh_token",
        ])
        guard let access = tok["access_token"] as? String else {
            forget()
            return nil
        }
        kept.access = access
        kept.until = Date().timeIntervalSince1970 + ((tok["expires_in"] as? Double) ?? 3600) - 60
        store(kept)
        return access
    }

    private func post(_ url: String, form: [String: String]) async throws -> [String: Any] {
        var req = URLRequest(url: URL(string: url)!)
        req.httpMethod = "POST"
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var parts = URLComponents()
        parts.queryItems = form.map { URLQueryItem(name: $0.key, value: $0.value) }
        req.httpBody = Data((parts.percentEncodedQuery ?? "").utf8)
        let (data, resp) = try await URLSession.shared.data(for: req)
        let obj = (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
        if let http = resp as? HTTPURLResponse, http.statusCode >= 400 {
            let why = (obj["error_description"] as? String) ?? (obj["error"] as? String) ?? ""
            throw Trouble.http(http.statusCode, why)
        }
        return obj
    }

    // ── Drive の API（使うぶんだけ）──

    private func api(_ pathAndQuery: String, method: String = "GET", body: Data? = nil,
                     contentType: String? = nil) async throws -> (Data, Int) {
        guard let access = try await token() else { throw Trouble.signedOut }
        var req = URLRequest(url: URL(string: apiUrl + pathAndQuery)!)
        req.httpMethod = method
        req.setValue("Bearer " + access, forHTTPHeaderField: "Authorization")
        if let contentType { req.setValue(contentType, forHTTPHeaderField: "Content-Type") }
        req.httpBody = body
        let (data, resp) = try await URLSession.shared.data(for: req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        if code >= 400 {
            var why = "HTTP \(code)"
            if let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let e = obj["error"] as? [String: Any], let m = e["message"] as? String { why = m }
            throw Trouble.http(code, why)
        }
        return (data, code)
    }
    private func json(_ pathAndQuery: String, method: String = "GET", body: Data? = nil,
                      contentType: String? = nil) async throws -> [String: Any] {
        let (data, _) = try await api(pathAndQuery, method: method, body: body, contentType: contentType)
        return (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
    }
    private static func q(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? s
    }

    /// **グループカレンダーを一枚作る**（依頼 532）。返すのは `(id, name)`。
    ///
    /// **押すのは一回。** サインインも、カレンダーの許可も、ここで面倒を見る ──
    /// 使う人に段取りを踏ませない。まだ Google に繋いでいなければ二つまとめて
    /// 訊く（「サインイン」と「カレンダーの許可」でブラウザを二度開かせない）。
    ///
    /// **予定の読み書きはこの道を通らない。** 作ったカレンダーは、Google の
    /// アカウントが iPhone に足してあれば端末のカレンダーに降りてくる ──
    /// そこから先は EventKit の仕事で、通信は要らない。
    func makeGroupCalendar(named name: String) async throws -> (id: String, name: String) {
        if !grants(Self.calScope) {
            let want = (load() == nil) ? Self.scope + " " + Self.calScope : Self.calScope
            _ = try await signIn(want: want)
            if !grants(Self.calScope) { throw Trouble.bad("カレンダーへのアクセスが許可されませんでした") }
        }
        let body = try JSONSerialization.data(withJSONObject: ["summary": name])
        let got = try await json("/calendar/v3/calendars", method: "POST", body: body,
                                 contentType: "application/json")
        guard let id = got["id"] as? String else { throw Trouble.bad("カレンダーを作成できませんでした") }
        return (id, (got["summary"] as? String) ?? name)
    }

    /// **グループカレンダーを消す**（依頼 535）。持ち主が消すと、グループの
    /// 人の画面からも消える ── 呼ぶ側は、押す前にそう言うこと。
    ///
    /// **もう向こうに無ければ、消し終わっている**（依頼 536）。人が Google の
    /// 画面で先に消していることはある ── そこで「Not Found」と言って止まると、
    /// **amber の憶えだけが永久に外せなくなる**。返すのは「本当に消したか」で、
    /// `false` は「もう無かった」。
    @discardableResult
    func dropGroupCalendar(_ id: String) async throws -> Bool {
        guard !id.isEmpty else { throw Trouble.bad("削除するカレンダーが特定できません") }
        do {
            _ = try await api("/calendar/v3/calendars/" + Self.q(id), method: "DELETE")
            return true
        } catch Trouble.http(let code, _) where code == 404 || code == 410 {
            return false
        }
    }

    /// そのカレンダーが、まだ向こうにあるか。**消されていたら nil。**
    func groupCalendar(_ id: String) async throws -> (id: String, name: String)? {
        do {
            let got = try await json("/calendar/v3/calendars/" + Self.q(id))
            guard let gid = got["id"] as? String else { return nil }
            return (gid, (got["summary"] as? String) ?? "")
        } catch Trouble.http(let code, _) where code == 404 {
            return nil
        }
    }

    private var homeId: String?
    private var dirIds: [String: String] = [:]

    /// amber の置き場所（無ければ作る）。
    private func home() async throws -> String {
        if let homeId { return homeId }
        let got = try await json("/drive/v3/files?q=" + Self.q("name='\(Self.homeName)' and mimeType='application/vnd.google-apps.folder' and trashed=false and 'root' in parents") + "&fields=files(id,name)")
        if let files = got["files"] as? [[String: Any]], let first = files.first, let id = first["id"] as? String {
            homeId = id
            return id
        }
        let meta: [String: Any] = ["name": Self.homeName, "mimeType": "application/vnd.google-apps.folder",
                                   "parents": ["root"], "appProperties": ["amber": "home"]]
        let made = try await json("/drive/v3/files?fields=id", method: "POST",
                                  body: try JSONSerialization.data(withJSONObject: meta), contentType: "application/json")
        homeId = made["id"] as? String ?? ""
        return homeId ?? ""
    }

    /// `仕事/入れ子` のフォルダ（無ければ作る・親から順に）。
    private func dir(_ relDir: String) async throws -> String {
        if relDir.isEmpty { return try await home() }
        if let id = dirIds[relDir] { return id }
        if dirIds.isEmpty {
            let got = try await json("/drive/v3/files?q=" + Self.q("appProperties has { key='amber' and value='dir' } and trashed=false") + "&fields=files(id,appProperties)&pageSize=1000")
            for f in got["files"] as? [[String: Any]] ?? [] {
                if let ap = f["appProperties"] as? [String: Any], let rel = ap["rel"] as? String, let id = f["id"] as? String { dirIds[rel] = id }
            }
            if let id = dirIds[relDir] { return id }
        }
        let up = relDir.contains("/") ? String(relDir[..<relDir.lastIndex(of: "/")!]) : ""
        let parent = try await dir(up)
        let meta: [String: Any] = ["name": relDir.split(separator: "/").last.map(String.init) ?? relDir,
                                   "mimeType": "application/vnd.google-apps.folder", "parents": [parent],
                                   "appProperties": ["amber": "dir", "rel": relDir]]
        let made = try await json("/drive/v3/files?fields=id", method: "POST",
                                  body: try JSONSerialization.data(withJSONObject: meta), contentType: "application/json")
        let id = made["id"] as? String ?? ""
        dirIds[relDir] = id
        return id
    }

    /// 向こうにあるノートと画像の一覧。
    func list() async throws -> [Remote] {
        var out: [Remote] = []
        var pageToken = ""
        repeat {
            let got = try await json("/drive/v3/files?q=" + Self.q("appProperties has { key='amber' and value='note' } and trashed=false")
                + "&fields=nextPageToken,files(id,name,md5Checksum,appProperties)&pageSize=1000"
                + (pageToken.isEmpty ? "" : "&pageToken=" + Self.q(pageToken)))
            for f in got["files"] as? [[String: Any]] ?? [] {
                let ap = f["appProperties"] as? [String: Any] ?? [:]
                guard let rel = ap["rel"] as? String, let id = f["id"] as? String else { continue }
                out.append(Remote(rel: rel, id: id, tag: ap["print"] as? String ?? (f["md5Checksum"] as? String ?? ""),
                                  by: ap["by"] as? String ?? ""))
            }
            pageToken = got["nextPageToken"] as? String ?? ""
        } while !pageToken.isEmpty
        return out
    }

    private static func mimeOf(_ name: String) -> String {
        let e = (name.split(separator: ".").last.map(String.init) ?? "").lowercased()
        return ["png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif", "webp": "image/webp",
                "heic": "image/heic", "bmp": "image/bmp", "svg": "image/svg+xml"][e] ?? "application/octet-stream"
    }

    /// 一本上げる（`id` があれば上書き）。字か画像。返すのは新しい id。
    func upload(rel: String, text: String? = nil, bytes: Data? = nil, print: String, id: String?) async throws -> String {
        let relDir = rel.contains("/") ? String(rel[..<rel.lastIndex(of: "/")!]) : ""
        let parent = id == nil ? try await dir(relDir) : nil
        let mime = bytes != nil ? Self.mimeOf(rel) : "text/markdown"
        var meta: [String: Any] = ["name": rel.split(separator: "/").last.map(String.init) ?? rel, "mimeType": mime,
                                   "appProperties": ["amber": "note", "rel": rel, "print": print, "by": by]]
        if let parent { meta["parents"] = [parent] }
        let boundary = "amber" + UUID().uuidString.replacingOccurrences(of: "-", with: "")
        var body = Data()
        body.append(Data(("--" + boundary + "\r\ncontent-type: application/json; charset=UTF-8\r\n\r\n").utf8))
        body.append(try JSONSerialization.data(withJSONObject: meta))
        body.append(Data(("\r\n--" + boundary + "\r\ncontent-type: " + mime + (bytes == nil ? "; charset=UTF-8" : "") + "\r\n\r\n").utf8))
        body.append(bytes ?? Data((text ?? "").utf8))
        body.append(Data(("\r\n--" + boundary + "--").utf8))
        let path = "/upload/drive/v3/files" + (id.map { "/" + Self.q($0) } ?? "") + "?uploadType=multipart&fields=id,md5Checksum"
        let got = try await json(path, method: id == nil ? "POST" : "PATCH", body: body,
                                 contentType: "multipart/related; boundary=" + boundary)
        return got["id"] as? String ?? (id ?? "")
    }

    /// 一本下ろす（字）。
    func download(_ id: String) async throws -> String {
        let (data, _) = try await api("/drive/v3/files/" + Self.q(id) + "?alt=media")
        return String(data: data, encoding: .utf8) ?? ""
    }
    /// 一本下ろす（画像・bytes のまま）。
    func downloadBytes(_ id: String) async throws -> Data {
        let (data, _) = try await api("/drive/v3/files/" + Self.q(id) + "?alt=media")
        return data
    }
    /// 向こうで消す ── ゴミ箱へ。
    func trash(_ id: String) async throws {
        _ = try await json("/drive/v3/files/" + Self.q(id), method: "PATCH",
                           body: try JSONSerialization.data(withJSONObject: ["trashed": true]), contentType: "application/json")
    }
    /// 向こうの名前（と親）を変える ── 同じ ID のまま。
    func rename(_ id: String, rel: String) async throws {
        let name = rel.split(separator: "/").last.map(String.init) ?? rel
        let relDir = rel.contains("/") ? String(rel[..<rel.lastIndex(of: "/")!]) : ""
        let now = try await json("/drive/v3/files/" + Self.q(id) + "?fields=parents")
        let want = try await dir(relDir)
        let had = now["parents"] as? [String] ?? []
        var query = ""
        if !want.isEmpty, !had.contains(want) {
            query = "&addParents=" + Self.q(want) + (had.isEmpty ? "" : "&removeParents=" + Self.q(had.joined(separator: ",")))
        }
        let meta: [String: Any] = ["name": name, "appProperties": ["rel": rel]]
        _ = try await json("/drive/v3/files/" + Self.q(id) + "?fields=id" + query, method: "PATCH",
                           body: try JSONSerialization.data(withJSONObject: meta), contentType: "application/json")
    }
}

/// ブラウザの小窓を、いまの窓の上に出すための係。
private final class Presenter: NSObject, ASWebAuthenticationPresentationContextProviding {
    static let shared = Presenter()
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        return scenes.flatMap { $0.windows }.first { $0.isKeyWindow } ?? ASPresentationAnchor()
    }
}

/// キーチェーンの読み書き（鍵は暗号化して置く ── 設定ファイルには書かない）。
enum Keychain {
    private static func query(_ key: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "amber", kSecAttrAccount as String: key]
    }
    static func get(_ key: String) -> Data? {
        var q = query(key)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: AnyObject?
        return SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess ? out as? Data : nil
    }
    static func set(_ key: String, _ data: Data) {
        SecItemDelete(query(key) as CFDictionary)
        var q = query(key)
        q[kSecValueData as String] = data
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(q as CFDictionary, nil)
    }
    static func delete(_ key: String) { SecItemDelete(query(key) as CFDictionary) }
}
