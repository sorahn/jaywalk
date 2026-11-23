# Additional icons

`diagram-flow-vertical.svg` comes from Lucide at commit `0669a71ff52d06b5f55b93b9b694e39038ad04ef`:
https://github.com/lucide-icons/lucide/blob/0669a71ff52d06b5f55b93b9b694e39038ad04ef/icons/diagram-flow-vertical.svg

`globe-code.svg` comes from Lucide at commit `b998e2892b90b88004d62da2d0b64dab9959a520`:
https://github.com/lucide-icons/lucide/blob/b998e2892b90b88004d62da2d0b64dab9959a520/icons/globe-code.svg

`sun-moon.svg` comes from Lucide at the same revision as `globe-code.svg`:
https://github.com/lucide-icons/lucide/blob/b998e2892b90b88004d62da2d0b64dab9959a520/icons/sun-moon.svg

The pinned GPUI Kit catalog does not yet include these icons. Each SVG is embedded
using the existing `Icon::data` API; geometry is unchanged, with its ISC notice
prepended so it is retained in the binary. See `LICENSE-LUCIDE`. Other interface
icons continue to use GPUI Kit's bundled catalog.
