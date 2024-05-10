import type {
  RolldownOutput,
  RolldownOutputAsset,
  RolldownOutputChunk,
  SourceMap,
} from '../types/rolldown-output'
import type { OutputBundle } from '../types/output-bundle'
import type {
  BindingOutputAsset,
  BindingOutputChunk,
  BindingOutputs,
} from '../binding'

function transformToRollupOutputChunk(
  chunk: BindingOutputChunk,
): RolldownOutputChunk {
  return {
    type: 'chunk',
    get code() {
      return chunk.code
    },
    set code(code: string) {
      chunk.code = code
    },
    fileName: chunk.fileName,
    get modules() {
      return Object.fromEntries(
        Object.entries(chunk.modules).map(([key, _]) => [key, {}]),
      )
    },
    get imports() {
      return chunk.imports
    },
    set imports(imports: string[]) {
      chunk.imports = imports
    },
    get dynamicImports() {
      return chunk.dynamicImports
    },
    exports: chunk.exports,
    isEntry: chunk.isEntry,
    facadeModuleId: chunk.facadeModuleId || null,
    isDynamicEntry: chunk.isDynamicEntry,
    get moduleIds() {
      return chunk.moduleIds
    },
    get map() {
      return chunk.map ? JSON.parse(chunk.map) : null
    },
    set map(map: SourceMap) {
      chunk.map = JSON.stringify(map)
    },
    sourcemapFileName: chunk.sourcemapFileName || null,
  }
}

function transformToRollupOutputAsset(
  asset: BindingOutputAsset,
): RolldownOutputAsset {
  return {
    type: 'asset',
    fileName: asset.fileName,
    get source() {
      return asset.source
    },
    set source(source: string) {
      asset.source = source
    },
  }
}

export function transformToRollupOutput(
  output: BindingOutputs,
): RolldownOutput {
  const { chunks, assets } = output
  const [firstChunk, ...restChunks] = chunks
  return {
    output: [
      transformToRollupOutputChunk(firstChunk),
      ...restChunks.map(transformToRollupOutputChunk),
      ...assets.map(transformToRollupOutputAsset),
    ],
  }
}

export function transformToOutputBundle(output: BindingOutputs): OutputBundle {
  return Object.fromEntries(
    transformToRollupOutput(output).output.map((item) => [item.fileName, item]),
  )
}
