/* amber ── Markdown のノートを、Swift へ。
 *
 * Two functions, because a C ABI has to be declared again on the Swift side
 * by hand and every extra symbol is another thing that can drift. Everything
 * amber に訊けることは全部 `amber_call` を通る:
 *
 *     let out = amber_call("notes", "{\"path\":\"/…/notes\"}")
 *     defer { amber_free(out) }
 *     let json = String(cString: out!)
 *
 * `method` and `params` are UTF-8; `params` is a JSON object (or "" for none).
 * The answer is always a JSON object and is never NULL — an error comes back
 * as {"error":"…"}, so there is no second way for a call to fail.
 *
 * **The caller owns the answer.** Every string `amber_call` returns must be
 * handed to `amber_free` exactly once. `amber_free(NULL)` is allowed, so an
 * error path may call it unconditionally.
 *
 * Methods: "version", "notes", "read", "write", "new".
 * See crates/amber-ffi/src/lib.rs for what each takes and returns.
 */
#ifndef AMBER_H
#define AMBER_H

#ifdef __cplusplus
extern "C" {
#endif

char *amber_call(const char *method, const char *params);
void amber_free(char *p);

#ifdef __cplusplus
}
#endif

#endif /* AMBER_H */
