import assert from "node:assert/strict"
import test from "node:test"
import { fileURLToPath } from "node:url"
import { createServer } from "vite"

test("drop traversal preserves files and rejects nested read failures", async () => {
  const server = await createServer({
    root: fileURLToPath(new URL("..", import.meta.url)),
    server: { middlewareMode: true },
    appType: "custom",
    logLevel: "silent",
  })
  try {
    const { extractFilesFromDataTransfer, traverseFileTree } =
      await server.ssrLoadModule("/src/pages/home/uploads/util.ts")
    const fileEntry = (name: string, fail = false) => ({
      name,
      isFile: true,
      isDirectory: false,
      file(resolve: (file: File) => void, reject: (error: Error) => void) {
        queueMicrotask(() =>
          fail ? reject(new Error("read failed")) : resolve(new File([name], name)),
        )
      },
    })
    const directoryEntry = (name: string, batches: any[][]) => ({
      name,
      isFile: false,
      isDirectory: true,
      createReader: () => ({
        readEntries(resolve: (entries: any[]) => void) {
          resolve(batches.shift() ?? [])
        },
      }),
    })

    const mixed = await extractFilesFromDataTransfer({
      items: [
        { kind: "string", getAsFile: () => null },
        { kind: "file", webkitGetAsEntry: () => null, getAsFile: () => new File(["right"], "right.txt") },
        { kind: "file", webkitGetAsEntry: () => directoryEntry("folder", [[fileEntry("inside.txt")], []]), getAsFile: () => null },
      ],
      files: [new File(["wrong"], "wrong.txt")],
    } as unknown as DataTransfer)

    assert.deepEqual(
      mixed.map(({ name }: { name: string }) => name),
      ["right.txt", "folder/inside.txt"],
    )
    await assert.rejects(
      traverseFileTree(directoryEntry("broken", [[fileEntry("bad.txt", true)]])),
      /read failed/,
    )
  } finally {
    await server.close()
  }
})
