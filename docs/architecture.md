# Architecture Overview

Rulist is a full-stack file management system consisting of a Rust backend and a SolidJS frontend.

## 1. Directory Layout & Boundaries

```
rulist/
├── src/                     # Backend: Rust (Actix-web / Tokio)
│   ├── server/              # HTTP API routes (auth, files, upload, stream)
│   ├── filesystem/          # Storage abstraction & ops (local_fs, cache, atomic mutations)
│   ├── preview/             # Preview processors for varied media types
│   └── db/                  # SQLite persistence & security credentials
├── web/                     # Frontend: SolidJS + Vite + TypeScript
│   ├── src/pages/home/      # File browser UI, toolbar actions, upload manager
│   ├── src/components/      # Preview modals, folder tree, icons
│   ├── src/store/           # Reactive SolidJS stores (files, session, navigation history)
│   ├── src/utils/           # API fetchers, StreamsSaver, zip streams
│   └── tests/               # Native node test suite (`node --test`)
├── scripts/                 # Automation & build toolchain
│   ├── check.sh             # Unified local & CI verification (fmt, clippy, tsc, tests)
│   ├── setup-hooks.sh       # Pre-commit hook registration (`.githooks`)
│   ├── build-frontend.sh    # Frontend build & distribution copy
│   └── build-release.sh     # Release binary bundling
└── tests/                   # Backend integration tests (auth, fs, security)
```

## 2. Cross-Stack Connections

- **File Operations**: Frontend `web/src/utils/api.ts` → Backend `src/server/files/` → `src/filesystem/ops.rs`.
- **Streaming & Download**: Frontend `web/src/utils/download.ts` & `zip-stream.js` ↔ Backend `src/server/stream.rs`.
- **Authentication**: Frontend `web/src/store/session.ts` ↔ Backend `src/server/auth.rs`.
