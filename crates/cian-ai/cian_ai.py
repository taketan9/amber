#!/usr/bin/env python3
"""cian AI helper — Azure OpenAI chat over Windows broker (WAM) auth.

This is a small, self-contained sibling of crmaine's python_backend: it reuses
the exact broker-auth client so cian can talk to the same Azure OpenAI endpoint,
but without any RAG/indexing. cian embeds this file in its binary and writes it
out to a cache dir at runtime, so there is nothing to ship separately.

Protocol (single request per process):

    python cian_ai.py --check
        Verify the required packages import for the given auth mode. Prints
        {"ok": true} / {"ok": false, "error": "..."} and exits 0/1. Does NOT
        contact the network or prompt for auth.

    python cian_ai.py            (request JSON on stdin)
        {"messages": [{"role": "...", "content": "..."}],
         "model": "...", "endpoint": "...", "api_version": "...",
         "auth_mode": "broker|apikey|mock", "api_key": "...",
         "api_base_url": "...", "max_tokens": 1024}
        Prints {"ok": true, "content": "..."} or {"ok": false, "error": "..."}.

Everything is one process per call; cian runs it on a worker thread. Azure
packages are imported lazily so `--check` for apikey/mock does not need them and
so a broken azure install cannot hang plain apikey use.
"""
import json
import sys


def read_request():
    raw = sys.stdin.read()
    return json.loads(raw) if raw.strip() else {}


def emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False))
    sys.stdout.flush()


def build_client(req):
    """Return an OpenAI-compatible client for the requested auth mode. Mirrors
    crmaine's _get_client so the broker path is identical."""
    auth = req.get("auth_mode", "broker")
    endpoint = req.get("endpoint", "")
    api_version = req.get("api_version", "2025-04-01-preview")
    api_key = req.get("api_key", "")
    api_base_url = req.get("api_base_url", "")

    if auth == "broker":
        try:
            import win32gui  # optional; gives the auth dialog a parent window
            parent_hwnd = win32gui.GetForegroundWindow()
        except Exception:
            parent_hwnd = 0
        from azure.identity.broker import InteractiveBrowserBrokerCredential
        from azure.identity import get_bearer_token_provider

        credential = InteractiveBrowserBrokerCredential(
            parent_window_handle=parent_hwnd,
            use_default_broker_account=True,
        )
        scope = "https://cognitiveservices.azure.com/.default"
        # An OpenAI-compatible gateway (APIM exposing /chat/completions with the
        # model in the body) is reached with a plain OpenAI client and the broker
        # token as the bearer key, rather than the Azure /openai/deployments/...
        # path. Opt in by setting api_base_url alongside broker auth.
        if api_base_url:
            from openai import OpenAI
            token = credential.get_token(scope).token
            return OpenAI(api_key=token, base_url=api_base_url)
        from openai import AzureOpenAI
        return AzureOpenAI(
            api_version=api_version,
            azure_endpoint=endpoint,
            azure_ad_token_provider=get_bearer_token_provider(credential, scope),
        )

    if auth == "apikey":
        from openai import OpenAI, AzureOpenAI
        if api_base_url:
            return OpenAI(api_key=api_key or "ollama", base_url=api_base_url)
        if endpoint:
            return AzureOpenAI(api_key=api_key, azure_endpoint=endpoint, api_version=api_version)
        return OpenAI(api_key=api_key)

    raise ValueError(f"unknown auth_mode: {auth}")


def do_check(auth):
    """Import what the auth mode needs, without contacting anything."""
    if auth == "mock":
        return
    import openai  # noqa: F401
    if auth == "broker":
        import azure.identity  # noqa: F401
        import azure.identity.broker  # noqa: F401


def main():
    if "--check" in sys.argv:
        # The auth mode can be passed as `--check <mode>`; default broker.
        auth = "broker"
        if "--check" in sys.argv:
            i = sys.argv.index("--check")
            if i + 1 < len(sys.argv):
                auth = sys.argv[i + 1]
        try:
            do_check(auth)
            emit({"ok": True, "pkgs": pkg_versions()})
            sys.exit(0)
        except Exception as e:  # noqa: BLE001
            emit({"ok": False, "error": f"{type(e).__name__}: {e}"})
            sys.exit(1)

    try:
        req = read_request()
        messages = req.get("messages", [])
        if req.get("auth_mode") == "mock":
            # Offline echo, for wiring up and testing cian without a network.
            last = messages[-1]["content"] if messages else ""
            emit({"ok": True, "content": f"[mock] {last}"})
            return
        client = build_client(req)
        model = req.get("model", "gpt-5-mini")
        kwargs = {"model": model, "messages": messages}
        if req.get("max_tokens"):
            kwargs["max_tokens"] = req["max_tokens"]
        resp = client.chat.completions.create(**kwargs)
        content = resp.choices[0].message.content or ""
        emit({"ok": True, "content": content})
    except Exception as e:  # noqa: BLE001
        emit({"ok": False, "error": describe_error(e, req)})
        sys.exit(1)


def pkg_versions():
    """openai / azure-identity-broker versions, so a URL that differs from a
    working client (different SDK major → different Azure path) is visible."""
    out = {}
    try:
        import openai
        out["openai"] = getattr(openai, "__version__", "?")
    except Exception:  # noqa: BLE001
        out["openai"] = "not importable"
    try:
        from importlib import metadata
        out["azure-identity-broker"] = metadata.version("azure-identity-broker")
    except Exception:  # noqa: BLE001
        pass
    return out


def describe_error(e, req):
    """A message that makes an HTTP failure actionable — the attempted URL, the
    model/deployment and api-version — so a 404 can be compared with a working
    setup. With Azure/broker auth the model IS the deployment name and lands in
    the URL path, so a wrong one yields exactly '404 Resource Not Found'."""
    base = f"{type(e).__name__}: {e}"
    url = None
    # openai>=1.0 status errors carry the httpx response/request.
    resp = getattr(e, "response", None)
    if resp is not None:
        try:
            url = str(resp.request.url)
        except Exception:  # noqa: BLE001
            url = None
    bits = [base]
    if url:
        bits.append(f"url={url}")
    bits.append(f"model/deployment={req.get('model', '')!r}")
    bits.append(f"api_version={req.get('api_version', '')!r}")
    bits.append(f"endpoint={req.get('endpoint', '')!r}")
    vers = pkg_versions()
    bits.append("pkgs=" + ", ".join(f"{k} {v}" for k, v in vers.items()))
    hint = getattr(e, "status_code", None)
    if hint == 404:
        bits.append(
            "hint: 404 can also mean the openai SDK version builds a different "
            "Azure URL than your working client — match its openai version "
            "(point cian.ai{ python=... } at the same interpreter), or check the "
            "deployment name / api-version in cian.ai{ model=..., api_version=... }."
        )
    return "  |  ".join(bits)


if __name__ == "__main__":
    main()
