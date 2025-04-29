import { StackFrame } from 'src/compiled/stacktrace-parser'
import { IPC, structuredError } from './index'
import type { Ipc as GenericIpc } from './index'
import { relative, isAbsolute, join, sep } from 'node:path'

type IpcIncomingMessage =
  | {
      type: 'evaluate'
      args: string[]
    }
  | {
      type: 'result'
      id: number
      error: string | null
      data: any | null
    }

type IpcOutgoingMessage =
  | {
      type: 'end'
      data: string | undefined
      duration: number
    }
  | {
      type: 'info'
      data: any
    }
  | {
      type: 'request'
      id: number
      data: any
    }

type IpcDependencies = {
  directories?: Array<[string, string]>
  filePaths?: string[]
  buildFilePaths?: string[]
}

export type IpcResolveOptions = {
  aliasFields: undefined | string[]
  conditionNames: undefined | string[]
  noPackageJson: boolean
  extensions: undefined | string[]
  mainFields: undefined | string[]
  noExportsField: boolean
  mainFiles: undefined | string[]
  noModules: boolean
  preferRelative: boolean
}

export type Ipc = {
  sendLog(logType: string, args: unknown[], trace?: StackFrame[]): void
  sendEmittedError(
    severity: 'warning' | 'error',
    error: string | Error
  ): Promise<void>
  sendDependencyInformation(message: IpcDependencies): void
  resolve(
    lookupPath: string,
    request: string,
    options: IpcResolveOptions
  ): Promise<string>
  sendError(error: Error | string): Promise<never>
}
const ipc = IPC as GenericIpc<IpcIncomingMessage, IpcOutgoingMessage>

// Patch process.env to track which env vars are read
const originalEnv = process.env
const readEnvVars = new Set<string>()
process.env = new Proxy(originalEnv, {
  get(target, prop) {
    if (typeof prop === 'string' && !readEnvVars.has(prop)) {
      // We register the env var as dependency on the
      // current transform and all future transforms
      // since the env var might be cached in module scope
      // and influence them all
      readEnvVars.add(prop)
    }
    return Reflect.get(target, prop)
  },
})

const contextDir = process.cwd()

// Normalize paths for Turbopack which deals with `/` delimited paths relative to the working directory
const toPath = (file: string) => {
  const relPath = relative(contextDir, file)
  if (isAbsolute(relPath)) {
    throw new Error(
      `Cannot depend on path (${file}) outside of root directory (${contextDir})`
    )
  }
  return sep !== '/' ? relPath.replaceAll(sep, '/') : relPath
}

// Reverse the normalization of `toPath`
const fromPath = (path: string) => {
  return join(contextDir || '', sep !== '/' ? path.replaceAll('/', sep) : path)
}

const queue: string[][] = []

export const run = async (
  moduleFactory: () => Promise<{
    init?: () => Promise<void>
    default: (ipc: Ipc, ...deserializedArgs: any[]) => any
  }>
) => {
  let nextId = 1
  const requests = new Map()
  // Defer sending these until the end of the task to improve efficiency.
  const logs: Array<{
    time: number
    logType: string
    args: unknown[]
    trace?: StackFrame[]
  }> = []
  let dependencyInfo:
    | {
        type: 'dependencies'
        envVariables?: string[]
        directories?: Array<[string, string]>
        filePaths?: string[]
        buildFilePaths?: string[]
      }
    | undefined = undefined

  function sendInfo(data: unknown): Promise<void> {
    return ipc.send({ type: 'info', data })
  }
  const internalIpc: Ipc = {
    sendError: (error: Error | string) => {
      return ipc.sendError(error)
    },
    sendDependencyInformation(message: IpcDependencies) {
      if (dependencyInfo) {
        throw new Error('`sendDependencyInformation` was already called?')
      }
      dependencyInfo = {
        type: 'dependencies',
        envVariables: Array.from(readEnvVars),
        directories: message.directories?.map(([path, glob]) => [
          toPath(path),
          glob,
        ]),
        filePaths: message.filePaths?.map(toPath),
        buildFilePaths: message.buildFilePaths?.map(toPath),
      }
    },
    sendLog(logType: string, args: unknown[], trace?: StackFrame[]): void {
      logs.push({ time: Date.now(), logType, args, trace })
    },
    sendEmittedError(
      severity: 'warning' | 'error',
      error: string | Error
    ): Promise<void> {
      return sendInfo({
        type: 'emittedError',
        severity,
        error: structuredError(error),
      })
    },
    async resolve(
      lookupPath: string,
      request: string,
      options: IpcResolveOptions
    ): Promise<string> {
      const id = nextId++
      let resolve, reject
      const promise = new Promise((res, rej) => {
        resolve = res
        reject = rej
      })
      requests.set(id, { resolve, reject })
      await ipc.send({
        type: 'request',
        id,
        data: {
          type: 'resolve',
          options,
          lookupPath: toPath(lookupPath),
          request,
        },
      })
      const unknownResult = await promise
      let result = unknownResult as { path: string }
      if (result && typeof result.path === 'string') {
        return fromPath(result.path)
      } else {
        throw Error('Expected `{ path: string }` from resolve request')
      }
    },
  }

  // Initialize module and send ready message
  let getValue: (ipc: Ipc, ...deserializedArgs: any[]) => any
  try {
    const module = await moduleFactory()
    if (typeof module.init === 'function') {
      await module.init()
    }
    getValue = module.default
    await ipc.sendReady()
  } catch (err) {
    await ipc.sendReady()
    await ipc.sendError(err as Error)
  }

  const flushInfos = () => {
    let promises = []
    promises.push(sendInfo({ type: 'log', logs }))
    promises.push(sendInfo(dependencyInfo))
    logs.length = 0
    dependencyInfo = undefined
    return Promise.all(promises)
  }

  // Queue handling
  let isRunning = false
  const run = async () => {
    while (queue.length > 0) {
      const args = queue.shift()!
      try {
        const value = await getValue(internalIpc, ...args)
        await flushInfos()
        await ipc.send({
          type: 'end',
          data:
            value === undefined ? undefined : JSON.stringify(value, null, 2),
          duration: 0,
        })
      } catch (e) {
        await flushInfos()
        await ipc.sendError(e as Error)
      }
    }
    isRunning = false
  }

  // Communication handling
  while (true) {
    const msg = await ipc.recv()

    switch (msg.type) {
      case 'evaluate': {
        queue.push(msg.args)
        if (!isRunning) {
          isRunning = true
          run()
        }
        break
      }
      case 'result': {
        const request = requests.get(msg.id)
        if (request) {
          requests.delete(msg.id)
          if (msg.error) {
            request.reject(new Error(msg.error))
          } else {
            request.resolve(msg.data)
          }
        }
        break
      }
      default: {
        console.error('unexpected message type', (msg as any).type)
        process.exit(1)
      }
    }
  }
}

export type { IpcIncomingMessage, IpcOutgoingMessage }
