import assert from "node:assert/strict"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("loading clears rejected work and preserves protected 401 redirects", async () => {
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
  })
  try {
    const { useLoading, useFetch } = await server.ssrLoadModule(
      "/src/hooks/useFetch.ts",
    )
    const failure = new Error("request failed")
    const [loading, run] = useLoading(async () => {
      throw failure
    })
    const pending = run()
    assert.equal(loading(), true)
    await assert.rejects(pending, (error) => error === failure)
    assert.equal(loading(), false)
    const [protectedLoading, protectedRun] = useFetch(async () => ({
      code: 401,
    }))
    await protectedRun()
    assert.equal(protectedLoading(), false)
    const session = await server.ssrLoadModule("/src/store/session.ts")
    session.setSessionExpired(true)
    assert.equal(protectedLoading(), true)
    session.setSessionExpired(false)
    const [loginLoading, loginRun] = useLoading(async () => ({ code: 401 }))
    await loginRun()
    assert.equal(loginLoading(), false)
    const completions: (() => void)[] = []
    const [overlapping, overlappingRun] = useLoading(
      () => new Promise<void>((resolve) => completions.push(resolve)),
    )
    const first = overlappingRun()
    const second = overlappingRun()
    completions[0]()
    await first
    assert.equal(overlapping(), true)
    completions[1]()
    await second
    assert.equal(overlapping(), false)
    const [successLoading, successRun] = useFetch(async () => ({ code: 200 }))
    assert.deepEqual(await successRun(), { code: 200 })
    assert.equal(successLoading(), false)
  } finally {
    await server.close()
  }
})

test("cancelling ZIP output cancels its pending file read and source", async () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "window")
  Object.defineProperty(globalThis, "window", { configurable: true, value: {} })
  try {
    const { default: createZip } = await import(
      new URL("../src/utils/zip-stream.js", import.meta.url).href
    )
    let fileReason: unknown
    let sourceReason: unknown
    const file = new ReadableStream({
      pull: () => new Promise(() => {}),
      cancel: (reason) => {
        fileReason = reason
      },
    })
    const zip = createZip({
      pull(writer: any) {
        writer.enqueue({ name: "pending.txt", stream: file })
        writer.close()
      },
      cancel(reason: unknown) {
        sourceReason = reason
      },
    })
    const reader = zip.getReader()
    assert.equal((await reader.read()).done, false)
    const pending = reader.read()
    await new Promise((resolve) => setImmediate(resolve))
    await reader.cancel("stopped")
    assert.equal((await pending).done, true)
    assert.equal(fileReason, "stopped")
    assert.equal(sourceReason, "stopped")
  } finally {
    if (descriptor) Object.defineProperty(globalThis, "window", descriptor)
    else Reflect.deleteProperty(globalThis, "window")
  }
})

test("ZIP output preserves names and bytes and rejects unrepresentable fields", async () => {
  const { default: createZip } = await import(
    new URL("../src/utils/zip-stream.js", import.meta.url).href
  )
  const { spawnSync } = await import("node:child_process")
  const entries = [
    { name: " space.txt ", bytes: new TextEncoder().encode("space") },
    { name: "目录/内容.txt", bytes: new TextEncoder().encode("中文\n") },
    { name: "empty.txt", bytes: new Uint8Array() },
  ]
  let index = 0
  const archive = createZip({
    pull(writer: any) {
      if (index === entries.length) {
        writer.close()
        return
      }
      const entry = entries[index++]
      writer.enqueue({
        name: entry.name,
        stream: new Blob([entry.bytes]).stream(),
      })
    },
  })
  const bytes = new Uint8Array(await new Response(archive).arrayBuffer())
  const decoded = spawnSync(
    "python3",
    [
      "-c",
      "import sys,io,zipfile; z=zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read())); assert z.testzip() is None; assert z.namelist()==[' space.txt ','目录/内容.txt','empty.txt']; assert z.read('目录/内容.txt')=='中文\\n'.encode(); assert z.read('empty.txt')==b''",
    ],
    { input: bytes, encoding: "utf8", timeout: 5000 },
  )
  assert.ifError(decoded.error)
  assert.equal(decoded.status, 0, decoded.stderr)
  const tooLong = createZip({
    pull(writer: any) {
      writer.enqueue({ name: "x".repeat(65536), stream: new Blob([]).stream() })
    },
  })
  await assert.rejects(
    new Response(tooLong).arrayBuffer(),
    /filename is too long/,
  )
})
