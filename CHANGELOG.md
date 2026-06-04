# Changelog

# 1.1.6
### Fixed
* Fix compilation error after `udp_server` upgraded to 1.1: `Bytes` returned by `recv()` is immutable, convert to `BytesMut` where mutation is needed (XOR decode, KCP input).
* Clippy: replace manual `div_ceil` and `abs_diff` with std methods in `kcp.rs`.

### Changed
* Pin `udp_server` dependency to `"1.1"` (was `"1"`).

### Added
* Bilingual README (English / 中文): features table, API overview, configuration reference, project structure.
* `CLAUDE.md`: codebase documentation for Claude Code agents.
* Python test scripts in `examples/kcpecho/`: `kcp_client.py` (happy-path echo test) and `kcp_probe.py` (comprehensive 5-probe suite).

# 1.1.5
### Features
* update 1.1.5

# 1.1.4
### Features
* fix kcp ctor current ts is 0 error

# 1.1.3
### Features
* update async-lock to 3.3

# 1.1.2
### Features
* use mutex Because RwLock doesn't mean much

# 1.1.1
### Features
* fix conv first make is 0

# 1.1.0
### Features
* add udp layer encode bytes

# 1.0.6
### Features
* Revise udp broken kcp peer log level to trace

# 1.0.5
### Features
* add AsyncRead for KcpReader
* add AsyncRead for KcpWriter

# 1.0.4
### Features
* check sn
* overwrite kcp peer

# 1.0.3
### Features
* fix sn error

# 1.0.2
### Features
* fix udp_server 1.0.2 compatibility issues

## 1.0.1
### Features
* optimize kcp peer update mode

## 1.0.0
### Features  
* new start
