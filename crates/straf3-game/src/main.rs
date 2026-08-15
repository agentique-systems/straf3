//! The straf3 executable: window, loop, glue.
//!
//! Stub — opening a window and running the loop lands in a later wave. Note
//! that per spec section 2, this binary is tuned on native Windows; under
//! WSLg it will run on a software adapter with no real vblank, so anything it
//! reports about frame pacing there is fiction.

fn main() {
    println!("straf3 {} — nothing to run yet.", env!("CARGO_PKG_VERSION"));
}
