//! Makes (or reads) the vault key twice and checks both reads agree; prints the biometric
//! this device offers. `cargo run --example vault` on a Mac; Android and Windows keep the
//! wrapped key in a file here.
use std::cell::RefCell;

struct File(RefCell<Option<Vec<u8>>>);
impl songbird_device::secure::Keep for File {
    fn load(&self) -> Option<Vec<u8>> {
        self.0.borrow().clone()
    }
    fn save(&self, w: &[u8]) {
        *self.0.borrow_mut() = Some(w.to_vec());
    }
}

fn main() {
    let keep = File(RefCell::new(None));
    let a = songbird_device::secure::vault_key(&keep);
    let b = songbird_device::secure::vault_key(&keep);
    match (a, b) {
        (Ok(a), Ok(b)) => println!("vault key: stable {}", *a == *b),
        (a, b) => println!("vault key: {:?} {:?}", a.err(), b.err()),
    }
    println!("biometrics: {:?}", songbird_device::biometrics::available());
}
