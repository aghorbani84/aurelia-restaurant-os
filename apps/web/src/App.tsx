import { FormEvent, useEffect, useMemo, useState } from 'react';
import {
  ArrowLeft, ArrowRight, Check, ChefHat, CircleHelp, LoaderCircle,
  Minus, Plus, Search, ShoppingBag, Store, UtensilsCrossed, X,
} from 'lucide-react';
import StaffPortal from './StaffPortal';

type Restaurant = { id: number; slug: string; name: string };
type MenuCategory = { id: number; restaurant_id: number; restaurant_name: string; name: string; slug: string };
type MenuItem = {
  id: number; restaurant_id: number; restaurant_name: string; category_id: number | null;
  category_name: string | null; name: string; description: string; price_minor: number; available: boolean;
};
type CreatedOrder = { id: number; order_number: string; status: string; total_minor: number };
type CartLine = MenuItem & { quantity: number };

const formatAmount = (value: number) => new Intl.NumberFormat('fa-IR', { maximumFractionDigits: 0 }).format(value);

const loadCart = (): Record<number, number> => {
  try {
    const saved = localStorage.getItem('aurelia-market-cart-v1');
    if (!saved) return {};
    const parsed = JSON.parse(saved) as Record<string, unknown>;
    return Object.fromEntries(Object.entries(parsed).filter(([id, quantity]) =>
      Number(id) > 0 && typeof quantity === 'number' && Number.isInteger(quantity) && quantity > 0 && quantity <= 99,
    ).map(([id, quantity]) => [Number(id), quantity as number]));
  } catch {
    return {};
  }
};

const foodEmoji = (category: string | null = '') => {
  const value = (category ?? '').toLowerCase();
  if (value.includes('پیتزا') || value.includes('pizza')) return '🍕';
  if (value.includes('برگر') || value.includes('burger')) return '🍔';
  if (value.includes('سوشی') || value.includes('sushi')) return '🍣';
  if (value.includes('کباب') || value.includes('grill')) return '🥩';
  if (value.includes('سالاد') || value.includes('salad')) return '🥗';
  if (value.includes('نوش') || value.includes('drink')) return '🥤';
  if (value.includes('دسر') || value.includes('dessert')) return '🍰';
  if (value.includes('ایرانی') || value.includes('rice')) return '🍚';
  return '🍽️';
};

function Storefront() {
  const [restaurants, setRestaurants] = useState<Restaurant[]>([]);
  const [items, setItems] = useState<MenuItem[]>([]);
  const [categories, setCategories] = useState<MenuCategory[]>([]);
  const [activeRestaurantId, setActiveRestaurantId] = useState<number | null>(null);
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [cart, setCart] = useState<Record<number, number>>(loadCart);
  const [pendingSwitchItem, setPendingSwitchItem] = useState<MenuItem | null>(null);
  const [customerName, setCustomerName] = useState('');
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');
  const [order, setOrder] = useState<CreatedOrder | null>(null);
  const [cartOpen, setCartOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      fetch('/api/restaurants').then((response) => {
        if (!response.ok) throw new Error('دریافت فهرست رستوران‌ها ناموفق بود.');
        return response.json() as Promise<Restaurant[]>;
      }),
      fetch('/api/menu/items').then((response) => {
        if (!response.ok) throw new Error('دریافت منو ناموفق بود.');
        return response.json() as Promise<MenuItem[]>;
      }),
      fetch('/api/menu/categories').then((response) => {
        if (!response.ok) throw new Error('دریافت دسته‌بندی‌ها ناموفق بود.');
        return response.json() as Promise<MenuCategory[]>;
      }),
    ])
      .then(([merchantList, menu, groups]) => {
        if (cancelled) return;
        setRestaurants(merchantList);
        setItems(menu);
        setCategories(groups);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : 'خطای ارتباط با سرور');
      })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    try { localStorage.setItem('aurelia-market-cart-v1', JSON.stringify(cart)); } catch { /* storage is optional */ }
  }, [cart]);

  useEffect(() => {
    if (loading || items.length === 0) return;
    setCart((current) => {
      const valid = Object.entries(current).filter(([id, quantity]) =>
        items.some((item) => item.id === Number(id) && quantity > 0),
      );
      const firstTenant = valid.length ? items.find((item) => item.id === Number(valid[0][0]))?.restaurant_id : null;
      const safe = Object.fromEntries(valid.filter(([id]) =>
        items.find((item) => item.id === Number(id))?.restaurant_id === firstTenant,
      ));
      if (Object.keys(safe).length === Object.keys(current).length) return current;
      return safe;
    });
  }, [items, loading]);

  useEffect(() => {
    if (!cartOpen && !pendingSwitchItem && !order) return;
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setCartOpen(false);
      setPendingSwitchItem(null);
      setOrder(null);
    };
    window.addEventListener('keydown', handleEscape);
    return () => window.removeEventListener('keydown', handleEscape);
  }, [cartOpen, pendingSwitchItem, order]);

  const activeRestaurant = restaurants.find((restaurant) => restaurant.id === activeRestaurantId) ?? null;
  const cartLines = useMemo<CartLine[]>(
    () => items.filter((item) => cart[item.id] > 0).map((item) => ({ ...item, quantity: cart[item.id] })),
    [items, cart],
  );
  const cartRestaurantId = cartLines[0]?.restaurant_id ?? null;
  const total = cartLines.reduce((sum, item) => sum + item.price_minor * item.quantity, 0);
  const itemCount = cartLines.reduce((sum, item) => sum + item.quantity, 0);

  const restaurantCards = useMemo(() => restaurants.map((restaurant) => {
    const restaurantItems = items.filter((item) => item.restaurant_id === restaurant.id);
    const cuisines = [...new Set(restaurantItems.map((item) => item.category_name).filter(Boolean))] as string[];
    const term = search.trim().toLowerCase();
    const matches = !term || restaurant.name.toLowerCase().includes(term)
      || restaurantItems.some((item) => item.name.toLowerCase().includes(term)
        || (item.category_name ?? '').toLowerCase().includes(term));
    return { ...restaurant, itemCount: restaurantItems.length, cuisines, matches };
  }), [restaurants, items, search]);

  const visibleRestaurantCards = restaurantCards.filter((restaurant) => restaurant.matches && restaurant.itemCount > 0);

  const marketplaceResults = useMemo(
    () => items.filter((item) => item.available && (!search.trim()
      || item.name.toLowerCase().includes(search.trim().toLowerCase())
      || (item.category_name ?? '').toLowerCase().includes(search.trim().toLowerCase())
      || item.restaurant_name.toLowerCase().includes(search.trim().toLowerCase()))),
    [items, search],
  );

  const restaurantItems = activeRestaurant ? items.filter((item) => item.restaurant_id === activeRestaurant.id) : [];
  const restaurantCategories = activeRestaurant
    ? [...new Set(categories.filter((category) => category.restaurant_id === activeRestaurant.id).map((category) => category.name))]
    : [];
  const visibleRestaurantItems = restaurantItems.filter((item) => item.available
    && (!activeCategory || item.category_name === activeCategory)
    && (!search.trim() || item.name.toLowerCase().includes(search.trim().toLowerCase())
      || (item.category_name ?? '').toLowerCase().includes(search.trim().toLowerCase())));

  const changeQuantity = (item: MenuItem, delta: number) => {
    setError('');
    setOrder(null);
    if (delta > 0 && !item.available) {
      setError('این غذا در حال حاضر موجود نیست.');
      return;
    }
    if (delta > 0 && cartRestaurantId !== null && cartRestaurantId !== item.restaurant_id) {
      setPendingSwitchItem(item);
      return;
    }
    setCart((current) => {
      const next = { ...current };
      const quantity = (next[item.id] ?? 0) + delta;
      if (quantity <= 0) delete next[item.id];
      else next[item.id] = Math.min(quantity, 99);
      return next;
    });
  };

  const switchRestaurantAndAdd = () => {
    if (!pendingSwitchItem) return;
    setCart({ [pendingSwitchItem.id]: 1 });
    setActiveRestaurantId(pendingSwitchItem.restaurant_id);
    setActiveCategory(null);
    setPendingSwitchItem(null);
    setError('');
  };

  const openRestaurant = (restaurantId: number) => {
    setActiveRestaurantId(restaurantId);
    setActiveCategory(null);
    setSearch('');
    setError('');
  };
  const backToMarketplace = () => {
    setActiveRestaurantId(null);
    setActiveCategory(null);
    setSearch('');
  };

  async function submitOrder(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError('');
    setOrder(null);
    if (!cartLines.length) { setError('سبد سفارش خالی است.'); return; }
    setSubmitting(true);
    try {
      const response = await fetch('/api/orders', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          customer_name: customerName.trim(),
          items: cartLines.map((item) => ({ menu_item_id: item.id, quantity: item.quantity })),
        }),
      });
      const body = await response.json();
      if (!response.ok) throw new Error(body.error ?? 'ثبت سفارش انجام نشد.');
      setOrder(body as CreatedOrder);
      setCart({});
      setCustomerName('');
      setCartOpen(false);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'ارتباط با سرور برقرار نشد.');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div dir="rtl" className="market-shell">
      <header className="market-header">
        <a className="market-brand" href="#home" onClick={backToMarketplace} aria-label="Aurelia home">
          <span className="brand-mark"><UtensilsCrossed size={19} /></span>
          <span><strong>AURELIA</strong><small>FOOD, FAIRLY FOUND</small></span>
        </a>
        <nav className="market-nav" aria-label="ناوبری اصلی">
          <a href="#restaurants">رستوران‌ها</a><a href="#how">چطور کار می‌کند</a><a href="/restaurant/apply">ثبت رستوران</a><a href="/staff">ورود کارکنان</a>
        </nav>
        <button className="header-cart" onClick={() => setCartOpen(true)} aria-label="باز کردن سبد">
          <ShoppingBag size={18} /><span>سبد</span><b>{formatAmount(itemCount)}</b>
        </button>
      </header>

      <main>
        <section id="home" className="market-hero">
          <div className="hero-content">
            <span className="hero-kicker"><span className="kicker-dot" /> انتخاب خوب، بدون شلوغی</span>
            <h1>غذای خوب.<br /><em>انتخاب آسان.</em></h1>
            <p>رستوران‌های مستقل، منوهای واقعی و قیمت‌های شفاف؛ بدون اینکه برای دیده‌شدن مجبور به تبلیغ باشید.</p>
            <div className="hero-search">
              <Search size={19} />
              <input value={search} onChange={(event) => { setSearch(event.target.value); setActiveRestaurantId(null); }}
                placeholder="نام غذا، رستوران یا مزه‌ای که می‌خواهید..." aria-label="جستجوی غذا یا رستوران" />
              {search && <button onClick={() => setSearch('')} aria-label="پاک کردن جستجو"><X size={16} /></button>}
              <a href="#restaurants" className="search-submit">جستجو</a>
            </div>
            <div className="hero-proof">
              <div><strong>{formatAmount(restaurants.length)}</strong><span>رستوران فعال</span></div><i />
              <div><strong>{formatAmount(items.length)}</strong><span>آیتم در منوها</span></div><i />
              <div><strong>شفاف</strong><span>قیمت و شرایط</span></div>
            </div>
          </div>
          <div className="hero-art" aria-hidden="true">
            <div className="hero-art-orbit orbit-one" /><div className="hero-art-orbit orbit-two" />
            <div className="hero-dish"><span>🍲</span></div>
            <div className="floating-food food-one">🥗</div><div className="floating-food food-two">🍕</div><div className="floating-food food-three">🍰</div>
            <div className="hero-art-caption"><span>DISCOVER</span><b>01 / 03</b></div>
          </div>
        </section>

        <section id="restaurants" className="discovery-section">
          <div className="section-title">
            <div><span className="section-kicker">DISCOVER YOUR NEXT FAVORITE</span>
              <h2>{activeRestaurant ? activeRestaurant.name : search ? 'نتایج جستجو' : 'رستوران‌های مستقل'}</h2>
              <p>{activeRestaurant ? 'منو را ببینید، انتخاب کنید و به سبد اضافه کنید.' : 'بدون رتبه‌بندی پولی؛ با انتخاب بر اساس منو و علاقه.'}</p>
            </div>
            {activeRestaurant && <button className="back-link" onClick={backToMarketplace}><ArrowRight size={16} /> بازگشت به رستوران‌ها</button>}
          </div>

          {loading ? <div className="market-state"><LoaderCircle className="spin" /> در حال بارگذاری...</div>
          : error && !items.length ? <div className="market-state error-state">{error}<small>اتصال API را بررسی کنید.</small></div>
          : activeRestaurant ? (
            <div className="restaurant-menu">
              <div className="restaurant-banner">
                <div className="restaurant-banner-art"><span>{foodEmoji(restaurantCategories[0])}</span></div>
                <div className="restaurant-banner-copy"><span className="section-kicker">MENU / {activeRestaurant.slug.toUpperCase()}</span>
                  <h3>{activeRestaurant.name}</h3><p>{formatAmount(restaurantItems.length)} انتخاب در منو</p>
                </div>
              </div>
              <div className="menu-filter-row">
                <button className={!activeCategory ? 'selected' : ''} onClick={() => setActiveCategory(null)}>همه</button>
                {restaurantCategories.map((category) => <button key={category} className={activeCategory === category ? 'selected' : ''}
                  onClick={() => setActiveCategory(category)}>{category}</button>)}
              </div>
              {visibleRestaurantItems.length === 0 ? <div className="market-state"><ChefHat size={25} /> موردی با این جستجو پیدا نشد.</div>
              : <div className="dish-grid">
                {visibleRestaurantItems.map((item, index) => <article className="dish-card" key={item.id}>
                  <div className={"dish-illustration tone-" + (index % 4)}>
                    <span className="dish-emoji">{foodEmoji(item.category_name)}</span><span className="dish-number">{String(index + 1).padStart(2, '0')}</span>
                    <span className="availability">موجود</span>
                  </div>
                  <div className="dish-card-content"><small>{item.category_name ?? 'منوی رستوران'}</small><h3>{item.name}</h3>
                    <p>{item.description || 'با مواد اولیه تازه و آماده‌سازی روز.'}</p>
                    <div className="dish-card-bottom"><strong>{formatAmount(item.price_minor)} <small>تومان*</small></strong>
                      <button onClick={() => changeQuantity(item, 1)} aria-label={"افزودن " + item.name}>
                        {cart[item.id] ? <span className="added-count">{formatAmount(cart[item.id])}</span> : <Plus size={18} />}
                      </button>
                    </div>
                  </div>
                </article>)}
              </div>}
            </div>
          ) : (
            <>
              {search && marketplaceResults.length > 0 && <div className="search-results-block">
                <div className="search-results-heading"><Search size={16} /> غذاهای مرتبط <span>{formatAmount(marketplaceResults.length)}</span></div>
                <div className="search-dishes">{marketplaceResults.slice(0, 6).map((item) => <button className="search-dish" key={item.id} onClick={() => openRestaurant(item.restaurant_id)}>
                  <span>{foodEmoji(item.category_name)}</span><div><strong>{item.name}</strong><small>{item.restaurant_name}</small></div>
                  <b>{formatAmount(item.price_minor)} <small>تومان*</small></b>
                </button>)}</div>
                <p className="search-hint">برای افزودن غذا به سبد، ابتدا منوی رستوران را باز کنید.</p>
              </div>}
              <div className="restaurant-grid">
                {visibleRestaurantCards.map((restaurant, index) => <article className="restaurant-card" key={restaurant.id}>
                  <button className="restaurant-card-open" onClick={() => openRestaurant(restaurant.id)}>
                    <div className={"restaurant-cover tone-" + (index % 4)}>
                      <span className="restaurant-cover-emoji">{foodEmoji(restaurant.cuisines[0])}</span>
                      <span className="restaurant-count">{formatAmount(restaurant.itemCount)} آیتم</span><span className="cover-mark">{String(index + 1).padStart(2, '0')}</span>
                    </div>
                    <div className="restaurant-card-body"><h3>{restaurant.name}</h3>
                      <p>{restaurant.cuisines.length ? restaurant.cuisines.slice(0, 3).join(' · ') : 'منوی رستوران'}</p>
                      <div className="restaurant-card-bottom"><span>مشاهده منو</span><ArrowLeft size={16} /></div>
                    </div>
                  </button>
                </article>)}
              </div>
              {!visibleRestaurantCards.length && !marketplaceResults.length && <div className="market-state"><CircleHelp size={25} /> نتیجه‌ای پیدا نشد؛ نام غذا یا رستوران دیگری را امتحان کنید.</div>}
              {!visibleRestaurantCards.length && !search && <div className="market-state empty-market"><Store size={26} /><strong>هنوز رستورانی فعال نشده</strong><span>با ثبت اولین رستوران، اینجا منوهای آن نمایش داده می‌شود.</span></div>}
            </>
          )}
        </section>

        <section id="how" className="trust-strip">
          <div><span className="trust-icon"><Check size={18} /></span><strong>قیمت واضح</strong><p>قیمت هر غذا قبل از سفارش دیده می‌شود.</p></div>
          <div><span className="trust-icon"><Store size={18} /></span><strong>رستوران مستقل</strong><p>رستوران برای دیده‌شدن مجبور به تبلیغ نیست.</p></div>
          <div><span className="trust-icon"><CircleHelp size={18} /></span><strong>بدون هزینه پنهان</strong><p>هزینه‌های پرداخت و ارسال هنوز در این نسخه فعال نیست.</p></div>
        </section>
      </main>

      <footer className="market-footer">
        <a className="market-brand" href="#home" onClick={backToMarketplace}><span className="brand-mark"><UtensilsCrossed size={18} /></span><span><strong>AURELIA</strong><small>FOOD, FAIRLY FOUND</small></span></a>
        <p>بازار غذا؛ منصفانه‌تر برای رستوران، ساده‌تر برای مشتری.</p>
        <small>© {new Date().getFullYear()} AURELIA · * واحد پول تومان، فرض اولیه بازار ایران</small>
      </footer>

      {cartOpen && <div className="cart-backdrop" onClick={() => setCartOpen(false)}>
        <aside className="cart-drawer" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()} aria-label="سبد سفارش">
          <div className="cart-drawer-header"><h2>سبد شما</h2><button onClick={() => setCartOpen(false)} aria-label="بستن"><X /></button></div>
          {cartLines.length === 0 ? <div className="cart-empty"><ShoppingBag size={30} /><p>سبد خالی است.</p><button onClick={() => setCartOpen(false)}>ادامه گشتن</button></div>
          : <>
            <div className="cart-restaurant-name"><Store size={15} /> {cartLines[0].restaurant_name}</div>
            {cartLines.map((item) => <div className="cart-line" key={item.id}>
              <div className="cart-line-art">{foodEmoji(item.category_name)}</div>
              <div className="cart-line-info"><strong>{item.name}</strong><small>{formatAmount(item.price_minor)} تومان*</small></div>
              <div className="quantity-control"><button onClick={() => changeQuantity(item, -1)} aria-label="کاهش"><Minus size={14} /></button>
                <b>{formatAmount(item.quantity)}</b><button onClick={() => changeQuantity(item, 1)} aria-label="افزایش"><Plus size={14} /></button></div>
            </div>)}
            <div className="cart-total"><span>جمع اقلام</span><strong>{formatAmount(total)} <small>تومان*</small></strong></div>
            <form className="cart-checkout" onSubmit={submitOrder}>
              <label htmlFor="customer-name">نام شما</label><input id="customer-name" value={customerName} onChange={(event) => setCustomerName(event.target.value)}
                maxLength={120} placeholder="برای ثبت درخواست" required />
              <p className="checkout-note">این نسخه پرداخت آنلاین، آدرس و تحویل را پشتیبانی نمی‌کند؛ فقط درخواست سفارش ثبت می‌شود.</p>
              {error && <p className="form-error" role="alert">{error}</p>}
              <button className="checkout-submit" type="submit" disabled={submitting || itemCount === 0 || cartLines.some((item) => !item.available)}>
                {submitting ? <LoaderCircle className="spin" /> : <Check size={18} />}{submitting ? 'در حال ثبت...' : 'ثبت درخواست'}
              </button>
            </form>
          </>}
        </aside>
      </div>}

      {pendingSwitchItem && <div className="switch-cart-alert" role="dialog" aria-modal="true" aria-label="تغییر رستوران سبد">
        <div className="switch-cart-card"><button className="switch-close" onClick={() => setPendingSwitchItem(null)} aria-label="بستن"><X size={17} /></button>
          <span className="switch-icon"><ShoppingBag /></span><h3>سبد فقط از یک رستوران است</h3>
          <p>برای سفارش از <strong>{pendingSwitchItem.restaurant_name}</strong> باید سبد فعلی را خالی کنید.</p>
          <div className="switch-actions"><button className="switch-confirm" onClick={switchRestaurantAndAdd}>پاک‌کردن سبد و تغییر رستوران</button>
            <button className="switch-cancel" onClick={() => setPendingSwitchItem(null)}>ادامه با سبد فعلی</button></div>
        </div>
      </div>}

      {order && <div className="order-confirmation" role="status"><div className="confirmation-card">
        <span className="confirmation-check"><Check size={24} /></span><h3>درخواست ثبت شد</h3><p>شماره پیگیری</p>
        <strong className="order-number">{order.order_number}</strong><small>وضعیت: در انتظار بررسی · پرداخت و تحویل در این نسخه فعال نیست.</small>
        <button onClick={() => setOrder(null)}>متوجه شدم</button>
      </div></div>}
    </div>
  );
}

function RestaurantApplicationPage() {
  const [form, setForm] = useState({
    restaurant_name: '',
    owner_username: '',
    password: '',
    contact_name: '',
    contact_phone: '',
    contact_email: '',
    city: '',
    address: '',
    cuisine: '',
    description: '',
  });
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');
  const [application, setApplication] = useState<{ application_id: number; restaurant_id: number; status: string } | null>(null);

  const update = (field: keyof typeof form, value: string) => setForm((current) => ({ ...current, [field]: value }));

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError('');
    setSubmitting(true);
    try {
      const response = await fetch('/api/restaurants/apply', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(form),
      });
      const body = await response.json();
      if (!response.ok) throw new Error(body.error || 'ارسال درخواست انجام نشد.');
      setApplication(body);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'ارتباط با سرور برقرار نشد.');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div dir="rtl" className="market-shell apply-shell">
      <header className="market-header">
        <a className="market-brand" href="/" aria-label="بازگشت به Aurelia">
          <span className="brand-mark"><UtensilsCrossed size={19} /></span>
          <span><strong>AURELIA</strong><small>FOOD, FAIRLY FOUND</small></span>
        </a>
        <a className="staff-back-link" href="/">بازگشت به بازار غذا</a>
      </header>
      <main className="restaurant-apply-page">
        <div className="apply-intro">
          <span className="hero-kicker"><span className="kicker-dot" /> همراه رستوران‌های مستقل</span>
          <h1>رستوران شما،<br /><em>در مسیر رشد.</em></h1>
          <p>اطلاعات اولیه را ثبت کنید. درخواست شما به‌صورت دستی بررسی می‌شود؛ هیچ هزینه یا تبلیغ اجباری برای ثبت درخواست وجود ندارد.</p>
          <div className="apply-benefits">
            <span><Check size={15} /> شرایط شفاف</span>
            <span><Check size={15} /> تبلیغ اجباری ندارد</span>
            <span><Check size={15} /> بررسی انسانی</span>
          </div>
        </div>
        <section className="apply-form-card">
          {application ? (
            <div className="apply-success">
              <span className="confirmation-check"><Check size={24} /></span>
              <h2>درخواست شما ثبت شد</h2>
              <p>شماره درخواست</p><strong className="order-number">APP-{formatAmount(application.application_id)}</strong>
              <p className="apply-success-note">حساب مالک تا تأیید درخواست غیرفعال است. پس از بررسی، حساب و رستوران فعال می‌شوند. در این نسخه، تأیید تماس و مدارک به‌صورت دستی انجام می‌شود.</p>
              <a className="checkout-submit" href="/">بازگشت به بازار غذا</a>
            </div>
          ) : (
            <>
              <div className="apply-card-heading"><span className="section-kicker">RESTAURANT APPLICATION</span><h2>فرم ثبت رستوران</h2><p>همه فیلدهای ستاره‌دار ضروری هستند.</p></div>
              <form className="restaurant-apply-form" onSubmit={submit}>
                <div className="apply-fields-grid">
                  <label>نام رستوران *<input value={form.restaurant_name} onChange={(event) => update('restaurant_name', event.target.value)} maxLength={160} required /></label>
                  <label>نام مسئول *<input value={form.contact_name} onChange={(event) => update('contact_name', event.target.value)} maxLength={120} required /></label>
                  <label>نام کاربری مالک *<input value={form.owner_username} onChange={(event) => update('owner_username', event.target.value.toLowerCase())} minLength={3} maxLength={32} pattern="[a-z0-9._-]+" autoComplete="username" required /><small>۳ تا ۳۲ حرف انگلیسی کوچک، عدد، نقطه، خط یا زیرخط</small></label>
                  <label>گذرواژه *<input type="password" value={form.password} onChange={(event) => update('password', event.target.value)} minLength={12} maxLength={128} autoComplete="new-password" required /><small>حداقل ۱۲ کاراکتر</small></label>
                  <label>شماره تماس *<input value={form.contact_phone} onChange={(event) => update('contact_phone', event.target.value)} inputMode="tel" maxLength={24} required /></label>
                  <label>ایمیل *<input type="email" value={form.contact_email} onChange={(event) => update('contact_email', event.target.value)} maxLength={254} required /></label>
                  <label>شهر *<input value={form.city} onChange={(event) => update('city', event.target.value)} maxLength={80} required /></label>
                  <label>نوع غذا *<input value={form.cuisine} onChange={(event) => update('cuisine', event.target.value)} maxLength={100} placeholder="مثلاً ایرانی، برگر، پیتزا" required /></label>
                  <label className="apply-address">آدرس کامل *<textarea value={form.address} onChange={(event) => update('address', event.target.value)} minLength={5} maxLength={500} rows={3} required /></label>
                  <label className="apply-description">توضیح کوتاه<textarea value={form.description} onChange={(event) => update('description', event.target.value)} maxLength={1000} rows={3} placeholder="درباره رستوران و سبک غذا..." /></label>
                </div>
                {error && <p className="form-error" role="alert">{error}</p>}
                <p className="apply-terms">با ارسال درخواست، موافق بررسی اطلاعات توسط تیم AURELIA هستید. حساب مالک و نمایش عمومی رستوران پس از تأیید فعال می‌شود.</p>
                <button className="checkout-submit" type="submit" disabled={submitting}>
                  {submitting ? <LoaderCircle className="spin" /> : <Store size={18} />}
                  {submitting ? 'در حال ارسال...' : 'ارسال برای بررسی'}
                </button>
              </form>
            </>
          )}
        </section>
      </main>
      <footer className="market-footer"><a className="market-brand" href="/"><span className="brand-mark"><UtensilsCrossed size={18} /></span><span><strong>AURELIA</strong><small>FOOD, FAIRLY FOUND</small></span></a><p>بازار غذا؛ منصفانه‌تر برای رستوران، ساده‌تر برای مشتری.</p></footer>
    </div>
  );
}

export default function App() {
  if (window.location.pathname === '/staff') return <StaffPortal />;
  if (window.location.pathname === '/restaurant/apply') return <RestaurantApplicationPage />;
  return <Storefront />;
}
