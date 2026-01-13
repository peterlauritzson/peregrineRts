use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Automatically profile a function when `perf_stats` feature is enabled.
/// 
/// This macro wraps the function body with timing code that prints
/// execution time on function exit. Compiles to nothing when the
/// `perf_stats` feature is disabled.
/// 
/// # Example
/// ```
/// #[profile]
/// pub fn my_expensive_function() {
///     // ... work ...
/// }
/// ```
/// 
/// # Future extensions
/// You can add sub-annotations like:
/// - `#[profile(time)]` - time-based profiling (default)
/// - `#[profile(memory)]` - memory allocation tracking
/// - `#[profile(detailed)]` - detailed breakdown
#[proc_macro_attribute]
pub fn profile(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    
    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let block = &input.block;
    let fn_name = &sig.ident;
    let fn_name_str = fn_name.to_string();
    
    let output = quote! {
        #(#attrs)*
        #vis #sig {
            #[cfg(feature = "perf_stats")]
            let _profile_timer = {
                let start = std::time::Instant::now();
                struct ProfileGuard {
                    name: &'static str,
                    start: std::time::Instant,
                }
                impl Drop for ProfileGuard {
                    fn drop(&mut self) {
                        let elapsed = self.start.elapsed();
                        if elapsed.as_millis() > 1 {
                            println!("[PERF] {}: {:?}", self.name, elapsed);
                        }
                    }
                }
                ProfileGuard {
                    name: #fn_name_str,
                    start,
                }
            };
            
            #block
        }
    };
    
    output.into()
}
