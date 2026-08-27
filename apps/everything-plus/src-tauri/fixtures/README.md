# XLS fixtures

`biff5_write.xls.b64` is the small legacy XLS fixture used by the pure-Rust
extractor tests. It is copied from the MIT-licensed calamine test corpus at
<https://github.com/tafia/calamine/blob/0.36.1/tests/biff5_write.xls> and is
kept as base64 text so the test fixture remains reviewable in source diffs.
The fixture is decoded only in tests; it is not bundled into the application.
