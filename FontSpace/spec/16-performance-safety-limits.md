# 16. Performance, Safety, and Limits

## 16.1 Expected scale

Documents are typically small. Optimize primarily for correctness and clarity. Still, avoid unnecessary per-frame allocation in GUI hot paths, and cache textures or batch the painter for whole-page previews.

## 16.2 File safety

Every document save uses atomic replacement: `path.tmp` → write → flush → rename over the destination. A failed load or migration must **never** silently overwrite the user's file. Optional backup/recovery may be layered on top. This atomic read/write is provided once by `fontspace-json` (`read_document` / `write_document`, over the canonical `save`/`load`) and used by both the CLI and the GUI, so the guarantee lives in exactly one place.

## 16.3 Workspace safety

Workspace/session JSON is auto-persisted (chapter 11) and may be regenerated if corrupted — losing it costs layout, not data. User-document corruption is never acceptable and is guarded by §16.2.

## 16.4 Resource limits

Define and enforce sensible bounds, well below `u16::MAX`, with errors that explain the exceeded limit:

- glyph width and height;
- number of character slots per set;
- pages per glyph set;
- total glyph pixels per document;
- export address width (address-bit count);
- output image size;
- fragment size.

Limits live in one place (a `Limits` value with documented defaults) so they are testable and adjustable, and so the validator (chapter 14) can cite them.
