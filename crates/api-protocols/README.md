# API protocol core

Pure protocol validation, bounded codecs and request projections shared by API
Playground and API Studio's native API component. The modules were extracted
from API Playground without changing their semantics. Native dialogs, DPAPI,
network connections and owned processes remain in the app command layers.

Run `cargo test -p api-protocols` from the repository root.
