import assert from "node:assert/strict"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("session reset clears navigation history, selection and pending work", async () => {
  const globals = ["document", "window", "localStorage"] as const
  const descriptors = globals.map((key) =>
    Object.getOwnPropertyDescriptor(globalThis, key),
  )
  Object.defineProperties(globalThis, {
    document: { configurable: true, value: { addEventListener() {} } },
    window: {
      configurable: true,
      value: {
        location: { origin: "http://localhost" },
        scrollY: 0,
        scroll() {},
      },
    },
    localStorage: {
      configurable: true,
      value: { getItem: () => null, setItem() {} },
    },
  })
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
  })
  try {
    const files = await server.ssrLoadModule("/src/store/files.ts")
    const session = await server.ssrLoadModule("/src/store/session.ts")
    const { resetSessionState } = await server.ssrLoadModule(
      "/src/store/reset.ts",
    )
    const { r } = await server.ssrLoadModule("/src/utils/request.ts")
    let requests = 0
    r.defaults.adapter = async (config: any) => {
      requests++
      return {
        data: {
          code: 200,
          data:
            config.url === "/fs/get"
              ? { name: "folder", is_dir: true }
              : {
                  content: [{ name: "old.txt", size: 3, is_dir: false }],
                  total: 1,
                },
        },
        status: 200,
        statusText: "OK",
        headers: {},
        config,
      }
    }
    // Observe the history limit through navigation requests, not the internal map.
    for (let index = 0; index <= 50; index++)
      await files.loadFolder(`/history-${index}`)
    const beforeRestore = requests
    await files.loadPath("/history-0")
    assert.equal(requests, beforeRestore)
    await files.loadFolder("/history-51")
    await files.loadPath("/history-1")
    assert.equal(requests, beforeRestore + 2)

    session.setCurrentUser({
      id: 1,
      username: "alice",
      role: 0,
      permission: 0,
      otp: false,
    })
    files.selectIndex(0, true)
    files.setDirectoryFilter("old")
    files.setLastClickedIndex(0)
    files.rememberDirectoryPath("/old", true)
    files.setUploadConfig({ overwrite: true })
    files.setShouldKeepState(true)
    const recovering = files.loadPath("/history-0")
    resetSessionState()
    await recovering

    assert.equal(session.currentUser(), null)
    assert.equal(files.fileStore.state, files.ViewState.Initial)
    assert.deepEqual(files.fileStore.file, {})
    assert.deepEqual(files.fileStore.files, [])
    assert.deepEqual(files.selectedFiles(), [])
    assert.equal(files.fileStore.total, 0)
    assert.equal(files.fileStore.page, 1)
    assert.equal(files.directoryFilter(), "")
    assert.equal(files.isKnownDirectoryPath("/old"), false)
    assert.deepEqual(files.uploadConfig, { overwrite: false })
    assert.equal(files.shouldKeepState(), false)
    const beforeReload = requests
    await files.loadFolder("/history-0")
    assert.equal(requests, beforeReload + 1)
  } finally {
    await server.close()
    globals.forEach((key, index) => {
      if (descriptors[index])
        Object.defineProperty(globalThis, key, descriptors[index]!)
      else Reflect.deleteProperty(globalThis, key)
    })
  }
})
