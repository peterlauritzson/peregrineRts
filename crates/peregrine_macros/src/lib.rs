use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, FnArg, Pat};

/// Automatically profile a function when `perf_stats` feature is enabled.
/// 
/// This macro wraps the function body with timing code that logs
/// execution time on function exit. Compiles to nothing when the
/// `perf_stats` feature is disabled.
/// 
/// # Features
/// - Auto-detects `tick: Res<SimTick>` parameter for tick-based logging
/// - Logs when duration > 1ms OR every 100 ticks (if tick available)
/// - Uses Bevy's `info!` logging instead of println
/// - Zero-cost abstraction when feature is disabled
/// 
/// # Example
/// ```
/// #[profile]
/// pub fn my_system(
///     query: Query<&Component>,
///     tick: Res<SimTick>,  // Auto-detected!
/// ) {
///     // ... work ...
/// }
/// ```
/// 
/// # Optional Parameters
/// ```
/// #[profile(2)]  // Custom threshold in milliseconds
/// pub fn expensive_function() { ... }
/// ```
#[proc_macro_attribute]
pub fn profile(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    
    // Parse optional threshold parameter
    let threshold_ms: u128 = if attr.is_empty() {
        1
    } else {
        attr.to_string().parse().unwrap_or(1)
    };
    
    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let block = &input.block;
    let fn_name_str = sig.ident.to_string();
    
    // Detect if function has a tick parameter (looking for patterns like `tick: Res<SimTick>`)
    let has_tick_param = sig.inputs.iter().any(|arg| {
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                if pat_ident.ident == "tick" {
                    // Check if type contains SimTick
                    let type_str = quote!(#pat_type.ty).to_string();
                    return type_str.contains("SimTick");
                }
            }
        }
        false
    });
    
    let profile_guard_def = if has_tick_param {
        quote! {
            struct ProfileGuard {
                name: &'static str,
                start: std::time::Instant,
                tick_value: u64,
            }
            impl Drop for ProfileGuard {
                fn drop(&mut self) {
                    let elapsed = self.start.elapsed();
                    if elapsed.as_millis() > #threshold_ms || (self.tick_value % 100 == 0) {
                        bevy::prelude::info!("[PERF] {}: {:?}", self.name, elapsed);
                    }
                }
            }
            ProfileGuard {
                name: #fn_name_str,
                start: std::time::Instant::now(),
                tick_value: tick.0,
            }
        }
    } else {
        quote! {
            struct ProfileGuard {
                name: &'static str,
                start: std::time::Instant,
            }
            impl Drop for ProfileGuard {
                fn drop(&mut self) {
                    let elapsed = self.start.elapsed();
                    if elapsed.as_millis() > #threshold_ms {
                        bevy::prelude::info!("[PERF] {}: {:?}", self.name, elapsed);
                    }
                }
            }
            ProfileGuard {
                name: #fn_name_str,
                start: std::time::Instant::now(),
            }
        }
    };
    
    let output = quote! {
        #(#attrs)*
        #vis #sig {
            #[cfg(feature = "perf_stats")]
            let _profile_timer = {
                #profile_guard_def
            };
            
            #block
        }
    };
    
    output.into()
}
