// StoreKit 2 for Songbird's device crate (D-0178): StoreKit 2 is Swift only, so this file is
// compiled into a static library by `build.rs` and called through C. It buys a product and
// hands back the signed transaction (JWS) the billing service verifies; it never keeps one.
import Foundation
import StoreKit

public typealias SBStoreDone = @convention(c) (UInt64, Int32, UnsafePointer<CChar>?) -> Void

/// Buys `product`: `done(id, 0, jws)` when purchased, 1 cancelled or pending, 2 not found,
/// 3 failed.
@_cdecl("sb_store_buy")
public func sbStoreBuy(_ product: UnsafePointer<CChar>, _ id: UInt64, _ done: SBStoreDone) {
    let productID = String(cString: product)
    Task {
        do {
            guard let p = try await Product.products(for: [productID]).first else {
                done(id, 2, nil)
                return
            }
            switch try await p.purchase() {
            case .success(let verification):
                if case .verified(let t) = verification {
                    await t.finish()
                }
                verification.jwsRepresentation.withCString { done(id, 0, $0) }
            case .userCancelled, .pending:
                done(id, 1, nil)
            @unknown default:
                done(id, 3, nil)
            }
        } catch {
            done(id, 3, nil)
        }
    }
}

/// The newest current subscription transaction this Apple account has (restore, or another
/// device's purchase): `done(id, 0, jws)`, or 2 when none.
@_cdecl("sb_store_current")
public func sbStoreCurrent(_ id: UInt64, _ done: SBStoreDone) {
    Task {
        var newest: VerificationResult<Transaction>?
        for await result in Transaction.currentEntitlements {
            if case .verified(let t) = result, t.productType == .autoRenewable {
                if newest == nil { newest = result }
                if case .verified(let n) = newest!, t.purchaseDate > n.purchaseDate { newest = result }
            }
        }
        if let r = newest {
            r.jwsRepresentation.withCString { done(id, 0, $0) }
        } else {
            done(id, 2, nil)
        }
    }
}
