import {
  VStack,
  Input,
  Heading,
  HStack,
  IconButton,
  Checkbox,
  Text,
  Badge,
  Progress,
  ProgressIndicator,
  Button,
  Stack,
} from "@hope-ui/solid"
import { createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { useFiles, useRouter } from "~/hooks"
import { can, mainColor, uploadConfig, setUploadConfig } from "~/store"
import {
  RiDocumentFolderUploadFill,
  RiDocumentFileUploadFill,
} from "solid-icons/ri"
import { bus, getFileSize, notify } from "~/utils"
import { UploadFileProps, StatusBadge } from "./types"
import {
  extractFilesFromDataTransfer,
  setUploadListenerActive,
  takePendingFiles,
} from "./util"
import { createUploadQueue } from "./queue"

const statusText: Record<string, string> = {
  pending: "Pending",
  uploading: "Uploading",
  backending: "Finalizing",
  success: "Success",
  error: "Error",
}

const UploadFile = (props: UploadFileProps & { onRetry?: () => void }) => (
  <VStack
    w="$full"
    spacing="$1"
    rounded="$lg"
    border="1px solid $neutral7"
    alignItems="start"
    p="$2"
    _hover={{ border: `1px solid ${mainColor()}` }}
  >
    <Text css={{ wordBreak: "break-all" }}>{props.path}</Text>
    <HStack spacing="$2" w="$full" justifyContent="space-between">
      <HStack spacing="$2">
        <Badge colorScheme={StatusBadge[props.status]}>
          {statusText[props.status] ?? props.status}
        </Badge>
        <Text>{getFileSize(props.speed)}/s</Text>
      </HStack>
      <HStack spacing="$2">
        <Show when={props.status === "error" && props.onRetry}>
          <Button
            compact
            size="xs"
            colorScheme="accent"
            onClick={() => props.onRetry?.()}
          >
            Retry
          </Button>
        </Show>
        <Text color="$neutral11">{getFileSize(props.size)}</Text>
      </HStack>
    </HStack>
    <Progress
      w="$full"
      trackColor="$info3"
      rounded="$full"
      value={props.progress}
      size="sm"
    >
      <ProgressIndicator color={mainColor()} rounded="$md" />
    </Progress>
    <Text color="$danger10">{props.msg}</Text>
  </VStack>
)

const Upload = () => {
  const { pathname } = useRouter()
  const { refresh } = useFiles()
  const [drag, setDrag] = createSignal(false)
  const [uploading, setUploading] = createSignal(false)
  const queue = createUploadQueue(() => {
    void refresh()
  })
  const uploadFiles = queue.state
  const allDone = () =>
    uploadFiles.uploads.every(({ status }) =>
      ["success", "error"].includes(status),
    )
  let fileInput!: HTMLInputElement
  let folderInput!: HTMLInputElement
  const handleAddFiles = (files: File[]) => {
    if (files.length === 0) return
    setUploading(true)
    queue.add(files, pathname(), uploadConfig.overwrite)
  }

  onMount(() => {
    setUploadListenerActive(true)
    const pending = takePendingFiles()
    if (pending.length > 0) handleAddFiles(pending)
  })

  const onUploadFiles = (files: File[]) => handleAddFiles(files)
  bus.on("upload_files", onUploadFiles)
  onCleanup(() => {
    queue.dispose()
    setUploadListenerActive(false)
    bus.off("upload_files", onUploadFiles)
  })

  return (
    <VStack w="$full" pb="$2" spacing="$2">
      <Show
        when={!uploading()}
        fallback={
          <>
            <HStack spacing="$2">
              <Button colorScheme="accent" onClick={queue.clearCompleted}>
                Clear completed
              </Button>
              <Show when={allDone()}>
                <Button onClick={() => setUploading(false)}>
                  Back to upload
                </Button>
              </Show>
            </HStack>
            <For each={uploadFiles.uploads}>
              {(upload) => (
                <UploadFile
                  {...upload}
                  onRetry={() => queue.retry(upload.id)}
                />
              )}
            </For>
          </>
        }
      >
        <Input
          type="file"
          multiple
          ref={fileInput}
          display="none"
          onChange={(e) =>
            handleAddFiles(Array.from(e.currentTarget.files ?? []))
          }
        />
        <Input
          type="file"
          multiple
          // @ts-ignore
          webkitdirectory
          ref={folderInput}
          display="none"
          onChange={(e) =>
            handleAddFiles(Array.from(e.currentTarget.files ?? []))
          }
        />
        <VStack
          w="$full"
          justifyContent="center"
          border={`2px dashed ${drag() ? mainColor() : "$neutral8"}`}
          rounded="$lg"
          spacing="$4"
          p="$6"
          minH="$56"
          onDragOver={(e: DragEvent) => {
            e.preventDefault()
            setDrag(true)
          }}
          onDragLeave={() => setDrag(false)}
          onDrop={async (e: DragEvent) => {
            e.preventDefault()
            e.stopPropagation()
            setDrag(false)
            let files: File[]
            try {
              files = await extractFilesFromDataTransfer(e.dataTransfer)
            } catch (error) {
              console.error("Failed to read dropped files", error)
              notify.error("Failed to read dropped files.")
              return
            }
            if (files.length === 0) {
              notify.warning("No files were dragged in.")
              return
            }
            handleAddFiles(files)
          }}
        >
          <Show when={!drag()} fallback={<Heading>Release to upload</Heading>}>
            <Heading size="lg" textAlign="center">
              Drag files here to upload, or click:
            </Heading>
            <HStack spacing="$4">
              <VStack spacing="$2" alignItems="center">
                <IconButton
                  compact
                  size="xl"
                  aria-label="Select folder"
                  colorScheme="accent"
                  icon={<RiDocumentFolderUploadFill size="1.2em" />}
                  onClick={() => folderInput.click()}
                />
                <Text fontSize="$sm" color="$neutral11" textAlign="center">
                  Select folder
                </Text>
              </VStack>
              <VStack spacing="$2" alignItems="center">
                <IconButton
                  compact
                  size="xl"
                  aria-label="Select files"
                  icon={<RiDocumentFileUploadFill size="1.2em" />}
                  onClick={() => fileInput.click()}
                />
                <Text fontSize="$sm" color="$neutral11" textAlign="center">
                  Select files
                </Text>
              </VStack>
            </HStack>
            <Stack
              spacing={{ "@initial": "$2", "@md": "$4" }}
              direction={{ "@initial": "column", "@md": "row" }}
            >
              <Show when={can("overwrite")}>
                <Checkbox
                  checked={uploadConfig.overwrite}
                  onChange={() =>
                    setUploadConfig({ overwrite: !uploadConfig.overwrite })
                  }
                >
                  Overwrite existing files
                </Checkbox>
              </Show>
            </Stack>
          </Show>
        </VStack>
      </Show>
    </VStack>
  )
}

export default Upload
