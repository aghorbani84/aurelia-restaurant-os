# Marketplace Foundation — Product & Architecture Decisions

**Status:** Draft baseline, 2026-11-23  
**Working product name:** AURELIA (not brand-locked)  
**Launch market assumption:** Persian-first, Iran; launch city and legal/payment details remain owner decisions.  
**Product:** Multi-restaurant food marketplace, not a single restaurant POS.

## 1. What exists today

The repository is a working early-stage restaurant system, not a marketplace yet:

- React/Vite frontend; Rust/Axum API; PostgreSQL 16; OpenResty gateway.
- Public menu and guest order endpoints.
- Staff login/session security, role-scoped order reads, audit log, and order status transitions.
- Current schema has global `menu_categories`, `menu_items`, `orders`; it does **not** establish restaurant ownership or tenant isolation.
- Customer storefront is a single-menu flow; restaurant registration, tenant-aware menus/orders, payments, settlement, courier integration, and production deployment are not complete.

**Decision:** retain the modular-monolith stack and extend it through additive, reviewed migrations. Do not prematurely split into microservices or rewrite the Rust backend.

## 2. Marketplace proposition

### Customers
- Browse without signup; location and filters are optional until needed.
- Search across restaurants and dishes; transparent final price before placing an order.
- One restaurant per cart initially; persistent cart, favorites, order history/tracking, and verified reviews phased in.
- No forced promotions, manipulative urgency, or fees revealed after confirmation.

### Restaurants
- Free/low-friction onboarding with verification before publishing.
- Control menu, pricing, availability, hours, capacity, prep-time estimates, delivery mode, and order acceptance.
- No paid placement required for discovery; no mandatory campaigns or discounts.
- A ledger-style payout view: order subtotal, commission, payment fee (if applicable), refunds/adjustments, delivery charges, and net amount.

### Marketplace
- Earn a transparent commission or optional subscription/software fee.
- Logistics is a separate module and cost line; do not subsidize it invisibly.
- Optional ads may exist only as clearly labelled, opt-in products and never affect organic ranking.

## 3. Competitive signal (directional, not contract terms)

Published/secondary 2026 references generally show marketplace delivery commissions in the **~25–30%** range, while pickup or restaurant self-delivery is often **~13–15%**. Rates vary by country, plan, member orders, and contract; these figures are not an Iran quote.

- Uber Eats Germany published pricing: 30% platform delivery, 14% self-delivery/pickup, with member-specific terms: https://merchants.ubereats.com/de/en/pricing/
- A November 2026 comparison for Lieferando/Uber Eats/Wolt: https://www.orderfront.de/vergleich/lieferando-uber-eats-wolt
- Commission and contract differences / extra costs: https://www.hospello.de/guides/lieferando-uber-eats-costs-germany

**Iran-specific signal:** Iranian reporting describes a Competition Council decision (meeting 740, 1404/02/16) finding exclusivity-related restaurant contract practices anti-competitive; later reporting says the appellate competition board upheld a ruling and also addressed misleading rating treatment. Treat this as a reported legal signal—not legal advice or a verified official ruling—until primary decisions are checked.

- Initial council decision report: https://www.ekhtebar.ir/%D8%B4%D9%88%D8%B1%D8%A7%DB%8C-%D8%B1%D9%82%D8%A7%D8%A8%D8%AA-%D8%B1%D9%88%DB%8C%D9%87-%D8%A7%D8%B3%D9%86%D9%BE%D9%81%D9%88%D8%AF-%D8%AF%D8%B1-%D9%86%D8%AD%D9%88%D9%87-%D8%A7%D9%86%D8%B9/
- Appellate decision coverage: https://digiato.com/iran-technology-news/snappfood-antitrust-ruling-score-manipulation-banned

**Product response:** merchant contracts must be non-exclusive, exit terms simple and fair, ranking rules auditable, verified ratings never secretly weighted by contract status, and sponsored placements visibly separated. The differentiator is predictable total cost and restaurant-owned customer relationship—not an unsustainably low headline rate. Local merchant interviews, payment rails, legal requirements, and city-level delivery costs still require primary research.

## 4. Pricing hypotheses to validate

Do not launch with a final commission before merchant interviews and unit-economics validation.

Test these offer structures with a founding cohort:
1. **Restaurant delivery / pickup:** low marketplace commission; payment processing disclosed separately if charged.
2. **Marketplace delivery:** commission plus a separately itemized, pass-through or contracted delivery price.
3. **Optional SaaS:** fixed fee for advanced operations/reporting, never required for basic listing or ranking.
4. **Optional promotion:** clearly marked sponsored placement, opt-in, separate from organic relevance ranking.

All merchant terms must be versioned, accepted explicitly, and visible before activation. Settlement dates and refund/chargeback rules must be explicit.

## 5. Organic discovery v0 (no pay-to-win)

Rank eligible restaurants using transparent signals; normalize each signal to 0–1:

- 30% successful fulfillment / low cancellation (quality and reliability)
- 25% Bayesian-adjusted verified customer rating (avoid small-sample extremes)
- 20% estimated delivery time (including prep time)
- 10% distance / serviceability
- 10% live capacity and availability
- 5% new-restaurant exploration

Use diversity-aware re-ranking to avoid showing many near-identical options. Reserve a small exploration opportunity among qualified restaurants (initial hypothesis: at least one new/under-exposed restaurant in the first 12 results where quality/serviceability thresholds are met). These weights and exploration quota are **hypotheses** to A/B test and recalibrate. Paid ads must not enter the organic score. Explain visible factors (“nearby”, “high reliability”, “new”) in UI.

## 6. MVP sequence

**Phase 0 — baseline (this PR):** product/architecture decisions and known gaps.

**Phase 1 — secure multi-tenancy foundation:**
- Restaurant entity, onboarding state, owner membership, tenant-scoped menu/categories.
- Tenant-aware order and order-line snapshots; tenant ownership enforced server-side.
- Legacy single-restaurant records mapped to a controlled default tenant.
- Add cross-tenant denial tests before any tenant UI is exposed.

**Phase 2 — restaurant operations:**
- Restaurant owner onboarding + verification queue.
- Menu CRUD, availability, hours, capacity, order inbox, accept/reject and status controls.
- Staff roles scoped to a restaurant; admin workflow and audit.

**Phase 3 — customer marketplace:**
- Restaurant discovery, search/filter, restaurant profile, multi-restaurant catalog (one-restaurant cart initially), price breakdown, address/checkout.
- Persistent cart and order tracking; notifications.

**Phase 4 — payment & settlements:**
- Provider abstraction; provider-confirmed payment/webhooks; idempotency; refunds/reconciliation; commission ledger and payout batches.
- Integrate a local provider only after legal, merchant, and account requirements are confirmed.

**Later:** courier fleet, third-party logistics, advanced loyalty, AI personalization, ads, multi-country and dedicated search infrastructure—only when validated.

## 7. Architecture and data rules

- Keep Rust/Axum + SQLx + PostgreSQL in a modular monolith.
- Domains: identity/access, restaurants, catalog, discovery, cart, orders, payments/ledger, delivery, reviews, promotions, support, admin.
- Every tenant-owned row has a restaurant scope. Authorization must derive tenant membership from the authenticated session, not client-supplied restaurant IDs.
- Tenant isolation must be enforced in queries and preferably strengthened with PostgreSQL RLS after migration/test design; never rely on frontend filtering.
- Order item snapshots preserve item name, options, price, and quantity at purchase time.
- Money remains integer minor units; define and document currency/minor-unit conventions per market.
- Use transactional writes, unique/idempotency keys for order/payment mutations, audit logs for sensitive operations.
- Add indexes based on query plans and real workloads; Redis/search service only when measured need justifies it.
- Stable, pinned dependencies and CI; secrets are never committed.

## 8. Unit economics gate

For every order, compute:

`Contribution/order = marketplace revenue - payment cost - refund/fraud loss - variable support - infrastructure variable cost - delivery subsidy`

Marketplace revenue = commission × eligible subtotal + disclosed fixed fees + optional SaaS/ad revenue. Delivery revenue and delivery expense are modeled separately.

Track GMV, take rate, net revenue, contribution margin, CAC, repeat rate, restaurant retention, customer retention, refund/cancellation rate, order success, and delivery cost/order. Calculate break-even orders/month as fixed monthly operating cost ÷ contribution/order. Do not forecast profitability until local costs and actual pilot data exist.

## 9. Launch constraints and open owner decisions

- Confirm first city and launch radius.
- Confirm legal entity, merchant contracts, tax/consumer obligations, and privacy policy with qualified local review.
- Select payment provider and settlement model after eligibility/account verification.
- Decide delivery: restaurant-run first, third-party integration, or a limited pilot fleet.
- Interview at least 15–20 restaurant operators and 20–30 target customers; document pain points and willingness-to-pay before locking fees.
- Choose brand name only after domain and trademark checks.

## 10. Definition of Phase 1 complete

- Tenant schema/migrations are additive and rollback/recovery documented.
- Default legacy restaurant is mapped without data loss.
- API authorization derives tenant scope from authenticated membership.
- Automated tests prove Restaurant A cannot read, mutate, or order Restaurant B’s menu/orders.
- CI passes on the exact commit; test evidence is linked.
- No customer-facing multi-tenant flow is released before these checks.
