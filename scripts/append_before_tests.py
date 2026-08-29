#!/usr/bin/env python3
"""Append Rust source *before* the test module, which is where it belongs.

Appending to the end of a file that ends in `#[cfg(test)] mod tests` puts the
new item after it, which Rust's own lint rejects — and it happened three times
in one session before this existed.
"""
import sys
path, block_file = sys.argv[1], sys.argv[2]
s = open(path).read()
block = open(block_file).read()
i = s.rfind('#[cfg(test)]')
if i < 0:
    open(path, 'w').write(s.rstrip('\n') + '\n' + block)
else:
    open(path, 'w').write(s[:i] + block.strip('\n') + '\n\n' + s[i:])
