use std::path::Path;
use wasmtime::*;

use crate::core::error::{NyayaError, Result};
use crate::runtime::manifest::AgentManifest;
use crate::runtime::receipt::ReceiptSigner;

/// WASM sandbox — loads and executes agent modules with fuel-metered execution
/// and permission-gated host function imports.
pub struct WasmSandbox {
    engine: Engine,
    #[allow(dead_code)]
    signer: ReceiptSigner,
}

/// Result of executing a WASM agent.
#[derive(Debug)]
pub struct ExecutionResult {
    /// Fuel consumed during execution
    pub fuel_consumed: u64,
    /// Whether execution completed successfully
    pub success: bool,
    /// Output captured from the agent (via log host functions)
    pub logs: Vec<String>,
}

/// Shared state accessible by host functions during execution.
pub struct HostState {
    pub manifest: AgentManifest,
    pub signer: ReceiptSigner,
    pub kv_store: std::collections::HashMap<String, Vec<u8>>,
    pub logs: Vec<String>,
    pub db_path: std::path::PathBuf,
}

impl WasmSandbox {
    /// Create a new sandbox with default engine configuration.
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true);

        // Set memory limits
        config.memory_guaranteed_dense_image_size(0);

        let engine = Engine::new(&config)
            .map_err(|e| NyayaError::Config(format!("WASM engine creation failed: {}", e)))?;

        let signer = ReceiptSigner::generate();

        Ok(Self { engine, signer })
    }

    /// Create a sandbox with a specific receipt signing key.
    pub fn with_signer(signer: ReceiptSigner) -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.memory_guaranteed_dense_image_size(0);

        let engine = Engine::new(&config)
            .map_err(|e| NyayaError::Config(format!("WASM engine creation failed: {}", e)))?;

        Ok(Self { engine, signer })
    }

    /// Load and execute a WASM module with the given manifest.
    pub fn execute(
        &self,
        wasm_path: &Path,
        manifest: &AgentManifest,
        db_path: &Path,
    ) -> Result<ExecutionResult> {
        // Load the WASM module
        let module = Module::from_file(&self.engine, wasm_path)
            .map_err(|e| NyayaError::Config(format!("Failed to load WASM module: {}", e)))?;

        // Create store with fuel limit
        let host_state = HostState {
            manifest: manifest.clone(),
            signer: ReceiptSigner::generate(), // Each execution gets its own signer
            kv_store: std::collections::HashMap::new(),
            logs: Vec::new(),
            db_path: db_path.to_path_buf(),
        };

        let mut store = Store::new(&self.engine, host_state);
        store
            .set_fuel(manifest.fuel_limit)
            .map_err(|e| NyayaError::Config(format!("Failed to set fuel: {}", e)))?;

        // Link host functions based on permissions
        let mut linker = Linker::new(&self.engine);
        self.link_host_functions(&mut linker, manifest)?;

        // Instantiate and run
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| NyayaError::Config(format!("WASM instantiation failed: {}", e)))?;

        // Call the _start function (WASI convention) or main
        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .or_else(|_| instance.get_typed_func::<(), ()>(&mut store, "main"));

        let success = match start {
            Ok(func) => {
                match func.call(&mut store, ()) {
                    Ok(()) => true,
                    Err(e) => {
                        // Check if it's a fuel exhaustion error
                        if e.to_string().contains("fuel") {
                            store.data_mut().logs.push(format!("Fuel exhausted: {}", e));
                            false
                        } else {
                            store
                                .data_mut()
                                .logs
                                .push(format!("Execution error: {}", e));
                            false
                        }
                    }
                }
            }
            Err(_) => {
                store
                    .data_mut()
                    .logs
                    .push("No _start or main function found".into());
                false
            }
        };

        let fuel_remaining = store
            .get_fuel()
            .map_err(|e| NyayaError::Config(format!("Failed to get fuel: {}", e)))?;
        let fuel_consumed = manifest.fuel_limit.saturating_sub(fuel_remaining);

        Ok(ExecutionResult {
            fuel_consumed,
            success,
            logs: store.into_data().logs,
        })
    }

    /// Link host functions into the WASM linker, gated by manifest permissions.
    fn link_host_functions(
        &self,
        linker: &mut Linker<HostState>,
        manifest: &AgentManifest,
    ) -> Result<()> {
        // Helper: read a string from WASM memory, returning an owned copy
        fn read_wasm_str(
            caller: &Caller<'_, HostState>,
            memory: &Memory,
            ptr: i32,
            len: i32,
        ) -> Option<String> {
            let data = memory.data(caller);
            let slice = data.get(ptr as usize..(ptr as usize + len as usize))?;
            std::str::from_utf8(slice).ok().map(|s| s.to_string())
        }

        fn read_wasm_bytes(
            caller: &Caller<'_, HostState>,
            memory: &Memory,
            ptr: i32,
            len: i32,
        ) -> Option<Vec<u8>> {
            let data = memory.data(caller);
            data.get(ptr as usize..(ptr as usize + len as usize))
                .map(|s| s.to_vec())
        }

        // Logging host functions
        if manifest.has_permission("log.info") || manifest.has_permission("log.error") {
            linker
                .func_wrap(
                    "env",
                    "log_info",
                    |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                        let msg = caller
                            .get_export("memory")
                            .and_then(|e| e.into_memory())
                            .and_then(|memory| read_wasm_str(&caller, &memory, ptr, len));
                        if let Some(msg) = msg {
                            caller.data_mut().logs.push(format!("[INFO] {}", msg));
                        }
                    },
                )
                .map_err(|e| NyayaError::Config(format!("Failed to link log_info: {}", e)))?;

            linker
                .func_wrap(
                    "env",
                    "log_error",
                    |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                        let msg = caller
                            .get_export("memory")
                            .and_then(|e| e.into_memory())
                            .and_then(|memory| read_wasm_str(&caller, &memory, ptr, len));
                        if let Some(msg) = msg {
                            caller.data_mut().logs.push(format!("[ERROR] {}", msg));
                        }
                    },
                )
                .map_err(|e| NyayaError::Config(format!("Failed to link log_error: {}", e)))?;
        }

        // KV store read
        if manifest.has_permission("kv.read") {
            linker
                .func_wrap(
                    "env",
                    "kv_get",
                    |mut caller: Caller<'_, HostState>,
                     key_ptr: i32,
                     key_len: i32,
                     _val_ptr: i32,
                     val_cap: i32|
                     -> i32 {
                        let key = caller
                            .get_export("memory")
                            .and_then(|e| e.into_memory())
                            .and_then(|memory| read_wasm_str(&caller, &memory, key_ptr, key_len));
                        let key = match key {
                            Some(k) => k,
                            None => return -1,
                        };
                        match caller.data().kv_store.get(&key) {
                            Some(val) => std::cmp::min(val.len(), val_cap as usize) as i32,
                            None => 0,
                        }
                    },
                )
                .map_err(|e| NyayaError::Config(format!("Failed to link kv_get: {}", e)))?;
        }

        // KV store write
        if manifest.has_permission("kv.write") {
            linker
                .func_wrap(
                    "env",
                    "kv_set",
                    |mut caller: Caller<'_, HostState>,
                     key_ptr: i32,
                     key_len: i32,
                     val_ptr: i32,
                     val_len: i32|
                     -> i32 {
                        let (key, val) = {
                            let memory =
                                match caller.get_export("memory").and_then(|e| e.into_memory()) {
                                    Some(m) => m,
                                    None => return -1,
                                };
                            let key = match read_wasm_str(&caller, &memory, key_ptr, key_len) {
                                Some(k) => k,
                                None => return -1,
                            };
                            let val = match read_wasm_bytes(&caller, &memory, val_ptr, val_len) {
                                Some(v) => v,
                                None => return -1,
                            };
                            (key, val)
                        };
                        caller.data_mut().kv_store.insert(key, val);
                        0
                    },
                )
                .map_err(|e| NyayaError::Config(format!("Failed to link kv_set: {}", e)))?;
        }

        Ok(())
    }
}

impl Default for WasmSandbox {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASM sandbox")
    }
}
