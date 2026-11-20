//! Buying a plan through the platform's store (D-0178): the App Store (StoreKit 2, through
//! `swift/Store.swift`) and Google Play (Play Billing in the Gradle host). The store's proof
//! goes to Songbird's billing service, which verifies it with the store and issues anonymous
//! tokens (§14.4); the store never learns the account, billing never learns the payment.
//!
//! Product IDs, the same in App Store Connect and the Play Console: `plus.monthly`,
//! `plus.yearly`, `family.monthly`, `family.yearly`.

#![forbid(unsafe_code)]

/// A store's proof of purchase, as the billing service takes it.
#[derive(Clone, PartialEq, Eq)]
pub enum Proof {
    /// StoreKit 2's signed transaction (JWS).
    Apple(String),
    /// Google Play's purchase token.
    Google(String),
}

impl std::fmt::Debug for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Apple(_) => "Apple(..)",
            Self::Google(_) => "Google(..)",
        })
    }
}

/// Why there is no purchase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Cancelled, or waiting for approval (Ask to Buy, a pending payment).
    Cancelled,
    /// The store doesn't sell this product (not set up, another country).
    NotFound,
    /// The store failed or isn't here.
    Failed,
}

/// The answer, on a thread of the system's choosing.
pub type Done = Box<dyn FnOnce(Result<Proof, Failure>) + Send>;

/// Whether this platform's store sells Songbird's plans.
#[must_use]
pub fn available() -> bool {
    cfg!(any(
        target_os = "ios",
        target_os = "macos",
        target_os = "android"
    ))
}

/// The product ID for a plan (`plus` or `family`) and interval (`month` or `year`).
#[must_use]
pub fn product(plan: &str, interval: &str) -> String {
    format!(
        "{plan}.{}",
        if interval == "year" {
            "yearly"
        } else {
            "monthly"
        }
    )
}

/// Buys `product` with the store's own sheet.
#[allow(unused_variables, reason = "platforms without a store")]
pub fn buy(product: &str, done: Done) {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    return crate::apple::store::buy(product, done);
    #[cfg(target_os = "android")]
    return crate::android::store_buy(product, done);
    #[allow(unreachable_code, reason = "other platforms")]
    done(Err(Failure::Failed));
}

/// The current subscription this store account has (bought on another device, or restored).
pub fn current(done: Done) {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    return crate::apple::store::current(done);
    #[cfg(target_os = "android")]
    return crate::android::store_current(done);
    #[allow(unreachable_code, reason = "other platforms")]
    done(Err(Failure::NotFound));
}

#[cfg(test)]
mod tests {
    #[test]
    fn product_ids_are_fixed() {
        assert_eq!(super::product("plus", "month"), "plus.monthly");
        assert_eq!(super::product("family", "year"), "family.yearly");
    }
}
