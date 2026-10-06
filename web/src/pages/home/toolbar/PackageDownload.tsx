import { useRouter } from "~/hooks"
import { downloadArchive } from "../../../utils/archive"
import { selectedFiles } from "~/store"
import { createSignal, For, Show, onCleanup } from "solid-js"
import {
  Box,
  Button,
  Heading,
  ModalBody,
  ModalFooter,
  Progress,
  ProgressIndicator,
  Text,
  VStack,
} from "@hope-ui/solid"

const PackageDownload = (props: { onClose: () => void }) => {
  const [cur, setCur] = createSignal("Initializing")
  const [status, setStatus] = createSignal(0)
  const [progress, setProgress] = createSignal({ current: 0, total: 0 })
  const { pathname } = useRouter()
  const rootPath = pathname()
  const selected = selectedFiles()

  const [fetchings, setFetchings] = createSignal<string[]>([])
  const controller = new AbortController()
  const { signal } = controller
  let disposed = false
  onCleanup(() => {
    disposed = true
    controller.abort()
  })

  void downloadArchive(rootPath, selected, {
    signal,
    onProgress: (progress) => {
      if (disposed) return
      if (progress.phase === "listing") {
        setCur("Fetching folder structure")
        setStatus(2)
      } else {
        setStatus(3)
        setProgress(progress)
        setCur(
          `Downloading files. Do not close or refresh this page. (${progress.current}/${progress.total})`,
        )
        setFetchings((prev) => [...prev.slice(-3), progress.name!])
      }
    },
  })
    .then(() => {
      if (disposed) return
      setCur("Download complete")
      setStatus(4)
    })
    .catch((error) => {
      if (disposed) return
      setCur(
        `Archive download failed: ${error instanceof Error ? error.message : String(error)}`,
      )
      setStatus(1)
    })

  return (
    <>
      <ModalBody>
        <VStack w="$full" alignItems="stretch" spacing="$3">
          <Heading size="base">{cur()}</Heading>
          <Show when={status() === 3 && progress().total > 0}>
            <Box w="$full">
              <Progress
                value={Math.round(
                  (progress().current / (progress().total || 1)) * 100,
                )}
                size="sm"
                rounded="$full"
              >
                <ProgressIndicator rounded="$full" bg="$accent9" />
              </Progress>
            </Box>
          </Show>
          <VStack w="$full" alignItems="start" spacing="$1">
            <For each={fetchings()}>
              {(name) => (
                <Text
                  size="xs"
                  color="$neutral10"
                  css={{ wordBreak: "break-all" }}
                >
                  {name}
                </Text>
              )}
            </For>
          </VStack>
        </VStack>
      </ModalBody>
      <Show when={[1, 4].includes(status())}>
        <ModalFooter>
          <Button colorScheme="info" onClick={props.onClose}>
            Close
          </Button>
        </ModalFooter>
      </Show>
    </>
  )
}

export default PackageDownload
