import { Accessor, createSignal } from "solid-js"
import { sessionExpired } from "../store/session"

export const useLoading = <Args extends unknown[], Result>(
  run: (...args: Args) => Promise<Result>,
  initial = false,
): [Accessor<boolean>, typeof run] => {
  const [loading, setLoading] = createSignal(initial)
  let active = 0
  return [
    loading,
    async (...args: Args) => {
      active++
      setLoading(true)
      try {
        return await run(...args)
      } finally {
        setLoading(--active > 0)
      }
    },
  ]
}

export const useFetch = <Args extends unknown[], Result>(
  run: (...args: Args) => Promise<Result>,
  initial = false,
): [Accessor<boolean>, typeof run] => {
  const [loading, fetch] = useLoading(run, initial)
  // Session expiry owns the protected-view state while routing to login.
  return [() => loading() || sessionExpired(), fetch]
}
