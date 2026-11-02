/* tslint:disable */
/* eslint-disable */
export function parse_certificate_file(bytes: Uint8Array): string;
export function certificate_png(item_json: string): Uint8Array;
export function start(canvas: HTMLCanvasElement): Promise<void>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly certificate_png: (a: number, b: number, c: number) => void;
  readonly parse_certificate_file: (a: number, b: number, c: number) => void;
  readonly start: (a: number) => number;
  readonly __wasm_bindgen_func_elem_1816: (a: number, b: number, c: number) => void;
  readonly __wasm_bindgen_func_elem_1635: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_1817: (a: number, b: number, c: number) => void;
  readonly __wasm_bindgen_func_elem_2241: (a: number, b: number, c: number) => void;
  readonly __wasm_bindgen_func_elem_2239: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_4408: (a: number, b: number, c: number, d: number) => void;
  readonly __wbindgen_export: (a: number, b: number) => number;
  readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export3: (a: number) => void;
  readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
  readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
