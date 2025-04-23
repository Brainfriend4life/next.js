import { CacheSignal } from '../cache-signal'
import { isThenable } from '../../../shared/lib/is-thenable'

/** Tracks all in-flight async imports and chunk loads. */
const moduleLoadingSignal = new CacheSignal()

export function trackPendingChunkLoad(promise: Promise<unknown>) {
  moduleLoadingSignal.trackRead(promise)
}

export function trackPendingImport(exportsOrPromise: unknown) {
  // requiring an async module returns a promise.
  // if it's sync, there's nothing to track.
  if (isThenable(exportsOrPromise)) {
    moduleLoadingSignal.trackRead(exportsOrPromise)
  }
}

/**
 * A top-level dynamic import (or chunk load):
 *   1. delays a render
 *   2. may reveal more caches
 * so if we see one, we make the `CacheSignal` wait for it to complete.
 *
 * We're not using `waitForPendingModules`,
 * because we might start and finish multiple batches of module loads while waiting for caches,
 * and `waitForPendingModules` would resolve after the first batch.
 * Instead, the import/chunk-load tracking mechanism will notify the cache signal
 * of each import/chunk-load that happens, and we'll delay `cacheSignal.cacheReady()` until all of them are done.
 *
 * There's a potential race if the page does some imports at the top level with a tasky delay:
 *
 * ```tsx
 *   const modulePromise = createPromiseWithResolvers();
 *
 *   void (async () => {
 *     const id = await uncachedFetch(); // tasky
 *     return import(`./foo/${id}`);
 *   })().then(
 *     (mod) => modulePromise.resolve(mod),
 *     (err) => modulePromise.reject(err)
 *   );
 *
 *   export default async function Page() {
 *     const mod = await modulePromise
 *     ...
 *   }
 * ```
 * In that case, if the `CacheSignal` wasn't already waiting for any other caches when the `import()` is called,
 * It may have already resolved `cacheReady()`, so we'd miss this in the prospective render
 * and likely fail in the actual prerender.
 */
export function trackPendingModules(cacheSignal: CacheSignal) {
  const unsubscribe = moduleLoadingSignal.subscribeToReads(cacheSignal)
  cacheSignal.cacheReady().then(unsubscribe)
}

/** Wait for currently pending imports and chunk loads to finish. */
export async function waitForPendingModules() {
  await moduleLoadingSignal.cacheReady()
}
