import { InvariantError } from '../../../shared/lib/invariant-error'
import { isThenable } from '../../../shared/lib/is-thenable'
import { trackPendingImport } from './track-module-loading.external'

/**
 * in DynamicIO, `import(...)` will be transformed into `trackDynamicImport(import(...))`.
 * A dynamic import is essentially a cached async function, except it's cached by the module system.
 *
 * The promises are tracked globally regardless of if the `import()` happens inside a render or outside of it.
 * When rendering, we can make the `cacheSignal` wait for all pending promises via `trackPendingModules`.
 * */
export function trackDynamicImport<TExports extends Record<string, any>>(
  modulePromise: Promise<TExports>
): Promise<TExports> {
  if (!isThenable(modulePromise)) {
    // We're expecting `import()` to always return a promise. If it's not, something's very wrong.
    throw new InvariantError('Expected the argument to be a promise')
  }

  // Even if we're inside render and have a `cacheSignal` available, always track the promise globally.
  // We do this because the `import()` promise might be cached in userspace:
  // (which is quite common for e.g. lazy initialization in libraries)
  //
  //   let promise;
  //   function doDynamicImportOnce() {
  //     if (!promise) {
  //       promise = import("...");
  //       // transformed into:
  //       // promise = trackDynamicImport(import("..."));
  //     }
  //     return promise;
  //   }
  //
  // In this case, if multiple prerenders depend on the promise, we should wait for it *in all of them*.
  // Tracking it using the `cacheSignal.trackRead()` would work for the first render,
  // but subsequent ones that re-use the promise wouldn't see it,
  // because `trackDynamicImport` would only be called once.
  trackPendingImport(modulePromise)

  return modulePromise
}

export function trackAsyncFunction<TFn extends (...args: any[]) => any>(
  name: string,
  func: TFn
): TFn {
  // it'd be confusing to see `__turbopack_require__` in a callstack twice, so disambiguate
  const wrapperName = 'tracked' + name

  return {
    [wrapperName]: function (this: unknown) {
      const result = func.call(this, arguments)
      if (isThenable(result)) {
        return trackDynamicImport(result)
      } else {
        return result
      }
    },
  }[wrapperName] as TFn
}
