package chat.songbird.store;

import android.app.Activity;

import com.android.billingclient.api.BillingClient;
import com.android.billingclient.api.BillingClientStateListener;
import com.android.billingclient.api.BillingFlowParams;
import com.android.billingclient.api.BillingResult;
import com.android.billingclient.api.PendingPurchasesParams;
import com.android.billingclient.api.ProductDetails;
import com.android.billingclient.api.Purchase;
import com.android.billingclient.api.QueryProductDetailsParams;
import com.android.billingclient.api.QueryPurchasesParams;

import java.util.Collections;
import java.util.List;

/**
 * Google Play Billing for Songbird's plans (D-0178). Buys a subscription with Play's own sheet
 * and hands the purchase token to Rust, which sends it to Songbird's billing service; billing
 * verifies and acknowledges it with Google. No account ID goes to Google.
 */
public final class PlayStore {
    private PlayStore() {}

    /** Answers: code 0 a token, 1 cancelled or pending, 2 not found, 3 failed. */
    public interface Answer {
        void done(int code, String token);
    }

    private static BillingClient client;
    private static Answer buying;

    private static void connect(Activity a, Runnable then, Answer fail) {
        if (client != null && client.isReady()) {
            then.run();
            return;
        }
        client = BillingClient.newBuilder(a.getApplicationContext())
                .setListener(PlayStore::updated)
                .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
                .build();
        client.startConnection(new BillingClientStateListener() {
            @Override
            public void onBillingSetupFinished(BillingResult r) {
                if (r.getResponseCode() == BillingClient.BillingResponseCode.OK) then.run();
                else fail.done(3, null);
            }

            @Override
            public void onBillingServiceDisconnected() {
                client = null;
            }
        });
    }

    private static void updated(BillingResult r, List<Purchase> purchases) {
        Answer a = buying;
        buying = null;
        if (a == null) return;
        int code = r.getResponseCode();
        if (code == BillingClient.BillingResponseCode.OK && purchases != null) {
            for (Purchase p : purchases) {
                if (p.getPurchaseState() == Purchase.PurchaseState.PURCHASED) {
                    a.done(0, p.getPurchaseToken());
                    return;
                }
            }
            a.done(1, null); // pending
        } else if (code == BillingClient.BillingResponseCode.USER_CANCELED) {
            a.done(1, null);
        } else {
            a.done(3, null);
        }
    }

    /** Buys subscription {@code product} (its first base plan). */
    public static void buy(Activity a, String product, Answer answer) {
        connect(a, () -> {
            QueryProductDetailsParams q = QueryProductDetailsParams.newBuilder()
                    .setProductList(Collections.singletonList(QueryProductDetailsParams.Product.newBuilder()
                            .setProductId(product)
                            .setProductType(BillingClient.ProductType.SUBS)
                            .build()))
                    .build();
            client.queryProductDetailsAsync(q, (r, result) -> {
                List<ProductDetails> list = result.getProductDetailsList();
                if (r.getResponseCode() != BillingClient.BillingResponseCode.OK || list == null || list.isEmpty()
                        || list.get(0).getSubscriptionOfferDetails() == null
                        || list.get(0).getSubscriptionOfferDetails().isEmpty()) {
                    answer.done(2, null);
                    return;
                }
                ProductDetails d = list.get(0);
                BillingFlowParams params = BillingFlowParams.newBuilder()
                        .setProductDetailsParamsList(Collections.singletonList(
                                BillingFlowParams.ProductDetailsParams.newBuilder()
                                        .setProductDetails(d)
                                        .setOfferToken(d.getSubscriptionOfferDetails().get(0).getOfferToken())
                                        .build()))
                        .build();
                a.runOnUiThread(() -> {
                    buying = answer;
                    BillingResult launched = client.launchBillingFlow(a, params);
                    if (launched.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                        buying = null;
                        answer.done(3, null);
                    }
                });
            });
        }, answer);
    }

    /** The current subscription's purchase token, if this Google account has one. */
    public static void current(Activity a, Answer answer) {
        connect(a, () -> client.queryPurchasesAsync(
                QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.SUBS).build(),
                (r, purchases) -> {
                    if (r.getResponseCode() == BillingClient.BillingResponseCode.OK && purchases != null) {
                        for (Purchase p : purchases) {
                            if (p.getPurchaseState() == Purchase.PurchaseState.PURCHASED) {
                                answer.done(0, p.getPurchaseToken());
                                return;
                            }
                        }
                    }
                    answer.done(2, null);
                }), answer);
    }
}
