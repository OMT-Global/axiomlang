/* Minimal Provider ABI v1 reference fixture: only opaque tokens cross the safe boundary. */
#include <stdint.h>
#include <stddef.h>
typedef uint64_t axiom_handle;
typedef struct { const uint8_t *data; size_t len; } axiom_borrowed_bytes;
typedef struct { uint8_t *data; size_t len; } axiom_owned_bytes;
typedef struct { uint32_t major, minor; uint64_t features; } axiom_provider_descriptor;
int axiom_provider_v1(axiom_provider_descriptor *out) { if (!out) return -1; out->major=1; out->minor=0; out->features=0; return 0; }
int axiom_provider_call(axiom_handle h, axiom_borrowed_bytes in, axiom_owned_bytes *out) { (void)in; if (!h || !out) return -1; out->data=NULL; out->len=0; return 0; }
int axiom_provider_close_handle(axiom_handle h) { return h ? 0 : -1; }
void axiom_provider_release_owned_buffer(axiom_owned_bytes v) { (void)v; }
