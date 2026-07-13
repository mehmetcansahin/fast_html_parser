# Benchmark fixtures

These files are immutable benchmark inputs. Throughput is always calculated
from the actual byte length, not from the approximate size in the filename.
Changing a fixture requires updating its byte length, SHA-256 digest, and the
observable benchmark signatures in the same change.

The repository history records when each fixture was added, but it does not
record when the real-world pages were captured. Capture dates are therefore
listed as unknown rather than inferred from commit dates.

| File | Kind | Bytes | SHA-256 | Known source | Capture date |
|---|---:|---:|---|---|---|
| `small_1kb.html` | Synthetic | 1,478 | `e19f021eca83dcf8b5c2051dc5eea145846304fcf51a16dee35410be0b9ff489` | Repository-generated HTML document | N/A |
| `medium_100kb.html` | Synthetic | 115,286 | `4b18d2e30adf0e65448df0fdbba0dba6a852e7296ab75beaf2eb8436eea0e427` | Repository-generated HTML document | N/A |
| `large_5mb.html` | Synthetic | 5,395,593 | `0e8cb49ac877163247d010ea8ac1f5aaf39d4af3173273cbe63a5333d3c195d5` | Repository-generated HTML document | N/A |
| `amazon.html` | Snapshot | 5,088 | `4af85243ae0939808462e294532a703ef20b876cde34f3ef630cbc4024676e06` | Amazon home page | Unknown |
| `hackernews.html` | Snapshot | 34,284 | `6e717995d1f65979a1a440a0d1d73d2a7e6d69c05454a48a98293b11b2d10456` | `https://news.ycombinator.com/` | Unknown |
| `github.html` | Snapshot | 301,093 | `8ca2eb2ec5663c0884bfb02c0681179e085889917dc9c3be31d1bba16e6f4484` | GitHub “Page not found” response; exact URL unavailable | Unknown |
| `stackoverflow.html` | Snapshot | 415,096 | `feb689ff4e3b62e27d70a999953b7da6a73e855d9d7645b4514b3bc1ea0ee6f2` | Stack Overflow newest questions tagged `rust` | Unknown |
| `wikipedia.html` | Snapshot | 589,673 | `cd1faa46cd5a9272424049af92204242df7b0dfcdb1e7d214c132c0c1308699f` | `https://en.wikipedia.org/wiki/Rust_(programming_language)` | Unknown |

Real-world snapshots remain fixed so historical benchmark comparisons use the
same bytes. Refreshes must be added as new, explicitly dated fixtures instead
of silently replacing an existing snapshot.
