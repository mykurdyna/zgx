---
section: fixed
---
- **Wrapped text remeasures when there is no width range to reuse** (zgx-text): a wrapped line can include trailing spaces whose measured width meets or exceeds the next wrap boundary. Its measured height is reused only when the range extends beyond both the input width and the widest measured line.
