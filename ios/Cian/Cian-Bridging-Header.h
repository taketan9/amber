/* What Swift is allowed to see of Rust: the two functions in amber.h and
   nothing else.

   **名前は `amber.h`。** cian から分かれた日に `amber-ffi` が出す header は
   `amber.h` になったが、ここだけ `cian.h` のまま残った ── ビルドは
   「'cian.h' file not found」で止まり、iOS が一度も建たなくなっていた。 */
#import "amber.h"
