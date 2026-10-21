import init from "contingency";

const WASM_FILE = "contingency_bg.wasm";

// For the <script> build, remember where this file was loaded from while the
// script is still executing; `document.currentScript` is null afterwards.
const scriptSrc: string | null =
  typeof document !== "undefined" && document.currentScript instanceof HTMLScriptElement
    ? document.currentScript.src || null
    : null;

/** Where to fetch the .wasm from when the trial does not say. */
export function defaultWasmUrl(): string {
  if (scriptSrc) {
    return new URL(WASM_FILE, scriptSrc).href;
  }
  try {
    return new URL(`./${WASM_FILE}`, import.meta.url).href;
  } catch {
    return new URL(WASM_FILE, document.baseURI).href;
  }
}

let loading: Promise<unknown> | null = null;

/** Instantiate the engine once per page. */
export function loadEngine(wasmUrl?: string | null): Promise<unknown> {
  if (!loading) {
    loading = init({ module_or_path: wasmUrl ?? defaultWasmUrl() }).catch((error) => {
      loading = null;
      throw error;
    });
  }
  return loading;
}
