import '../../server/web/globals'

import { adapter } from '../../server/web/adapter'
import { IncrementalCache } from '../../server/lib/incremental-cache'
import { wrapApiHandler } from '../../server/api-utils'

// Import the userland code.
import userHandler from 'VAR_USERLAND'
import { isDynamicRoute } from '../../shared/lib/router/utils'
import { getRouteMatcher } from '../../shared/lib/router/utils/route-matcher'
import { getRouteRegex } from '../../shared/lib/router/utils/route-regex'
import type { I18NConfig } from '../../server/config-shared'

const page = 'VAR_DEFINITION_PAGE'

if (typeof userHandler !== 'function') {
  throw new Error(
    `The Edge Function "pages${page}" must export a \`default\` function`
  )
}

export default function handler(
  req: Request,
  ctx: {
    waitUntil: (prom: Promise<void>) => void
  }
) {
  let params: Record<string, string[] | string | undefined> | undefined

  // TODO: should this process rewrite params same as non-edge
  // this does not currently so keeping existing behavior of only
  // parsing dynamic route params
  if (isDynamicRoute(page)) {
    const { pathname } = new URL(req.url)
    const match = getRouteMatcher(getRouteRegex(page))(pathname)

    if (match) {
      params = match
    }
  }

  return adapter({
    request: {
      headers: Object.fromEntries(req.headers.entries()),
      method: req.method,
      nextConfig: {
        basePath: process.env.__NEXT_ROUTER_BASEPATH,
        i18n: process.env.__NEXT_I18N_CONFIG as any as I18NConfig | null,
        trailingSlash: process.env.__NEXT_TRAILING_SLASH as any as boolean,
        experimental: {},
      },
      page: {
        name: page,
        params,
      },
      url: req.url,
      body: req.body || undefined,
      /** passed in when running in edge runtime sandbox */
      signal: new AbortController().signal,
      waitUntil: ctx.waitUntil,
    },
    IncrementalCache,
    page,
    handler: wrapApiHandler(page, userHandler),
  })
}
