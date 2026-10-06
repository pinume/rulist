# CODING_STANDARDS.md

本文件仅供审查阶段（Review Agent）查阅，记录 linter 与编译器无法自动校验的架构判断准则。实现阶段（Implementation Agent）无需默认加载。

## 1. 流式传输与缓存边界 (Streaming & Security)
- **无鉴权缓存隔离**：带签名、包含 JWT 或用于权限校验的流式响应（`src/server/stream.rs`）严禁启用客户端/中间人 HTTP 缓存（强制 `Cache-Control: no-store` 或私有防泄露策略）。
- **签名链路独立**：任何公开签名的资源 URL 不得反向泄露系统底层绝对路径，始终保持对外路径不透明。

## 2. 前端响应式状态 (SolidJS State Management)
- **单向数据流**：全局状态（`web/src/store/`）由专用 action 函数驱动，禁止在组件视图逻辑中多处随意直接解构赋值或分散 patch。
- **纯粹派生**：`createMemo` 与计算 getter 内严禁产生隐式异步副作用或引发循环触发的状态更新。

## 3. 文件系统原子性与局部失败隔离 (Filesystem Operations)
- **写时暂存与原子替换**：破坏性覆写操作（`src/filesystem/ops.rs`、`src/filesystem/local.rs`）优先采用临时文件写入 + 原子性重命名，避免写中断损坏目标文件。
- **批量操作故障隔离**：批量重命名/移动/删除若部分条目失败，已成功条目保持有效，失败条目需收集独立错误信息，严禁半路 panic 或未隔离状态回滚。
