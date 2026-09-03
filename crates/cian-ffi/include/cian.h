/* cian's notes, for Swift.
 *
 * Two functions, because a C ABI has to be declared again on the Swift side
 * by hand and every extra symbol is another thing that can drift. Everything
 * cian can be asked goes through `cian_call`:
 *
 *     let out = cian_call("notes", "{\"path\":\"/…/notes\"}")
 *     defer { cian_free(out) }
 *     let json = String(cString: out!)
 *
 * `method` and `params` are UTF-8; `params` is a JSON object (or "" for none).
 * The answer is always a JSON object and is never NULL — an error comes back
 * as {"error":"…"}, so there is no second way for a call to fail.
 *
 * **The caller owns the answer.** Every string `cian_call` returns must be
 * handed to `cian_free` exactly once. `cian_free(NULL)` is allowed, so an
 * error path may call it unconditionally.
 *
 * Methods: "version", "notes", "read", "write", "new".
 * See crates/cian-ffi/src/lib.rs for what each takes and returns.
 */
#ifndef CIAN_H
#define CIAN_H

#ifdef __cplusplus
extern "C" {
#endif

char *cian_call(const char *method, const char *params);
void cian_free(char *p);

#ifdef __cplusplus
}
#endif

#endif /* CIAN_H */
