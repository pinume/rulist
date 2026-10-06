export type ZipEntry = { name: string; stream: ReadableStream<Uint8Array> }
export type ZipWriter = {
  enqueue: (entry: ZipEntry) => void
  close: () => void
}

export default function createZip(source: {
  pull: (writer: ZipWriter) => void | Promise<void>
  cancel?: (reason: unknown) => void
}): ReadableStream<Uint8Array>
