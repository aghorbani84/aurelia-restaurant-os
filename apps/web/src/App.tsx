import { FormEvent, useEffect, useMemo, useState } from 'react';
import { ArrowDown, ArrowLeft, Check, ChefHat, LoaderCircle, Minus, Plus, ShoppingBag, UtensilsCrossed } from 'lucide-react';
import Scene from './Scene';

type MenuCategory = { id: number; name: string; slug: string };
type MenuItem = {
  id: number;
  category_id: number | null;
  category_name: string | null;
  name: string;
  description: string;
  price_minor: number;
  available: boolean;
};
type CreatedOrder = { id: number; order_number: string; status: string; total_minor: number };

const formatAmount = (value: number) =>
  new Intl.NumberFormat('fa-IR', { maximumFractionDigits: 0 }).format(value);

export default function App() {
  const [items, setItems] = useState<MenuItem[]>([]);
  const [categories, setCategories] = useState<MenuCategory[]>([]);
  const [activeCategory, setActiveCategory] = useState<number | 'all'>('all');
  const [cart, setCart] = useState<Record<number, number>>({});
  const [customerName, setCustomerName] = useState('');
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');
  const [order, setOrder] = useState<CreatedOrder | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      fetch('/api/menu/items').then((response) => {
        if (!response.ok) throw new Error('دریافت منو ناموفق بود.');
        return response.json() as Promise<MenuItem[]>;
      }),
      fetch('/api/menu/categories').then((response) => {
        if (!response.ok) throw new Error('دریافت دسته‌بندی‌ها ناموفق بود.');
        return response.json() as Promise<MenuCategory[]>;
      }),
    ])
      .then(([menu, groups]) => {
        if (cancelled) return;
        setItems(menu);
        setCategories(groups);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : 'خطای ارتباط با سرور');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  const visibleItems = useMemo(
    () => items.filter((item) => item.available &&
      (activeCategory === 'all' || item.category_id === activeCategory)),
    [items, activeCategory],
  );
  const cartLines = useMemo(
    () => items.filter((item) => cart[item.id] > 0).map((item) => ({
      ...item,
      quantity: cart[item.id],
    })),
    [items, cart],
  );
  const total = cartLines.reduce((sum, item) => sum + item.price_minor * item.quantity, 0);
  const itemCount = cartLines.reduce((sum, item) => sum + item.quantity, 0);

  const changeQuantity = (id: number, delta: number) => {
    setCart((current) => {
      const next = { ...current };
      const quantity = (next[id] ?? 0) + delta;
      if (quantity <= 0) delete next[id];
      else next[id] = Math.min(quantity, 99);
      return next;
    });
    setOrder(null);
  };

  async function submitOrder(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError('');
    setOrder(null);
    if (!cartLines.length) {
      setError('سبد سفارش خالی است.');
      return;
    }
    setSubmitting(true);
    try {
      const response = await fetch('/api/orders', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          customer_name: customerName.trim(),
          items: cartLines.map((item) => ({
            menu_item_id: item.id,
            quantity: item.quantity,
          })),
        }),
      });
      const body = await response.json();
      if (!response.ok) throw new Error(body.error ?? 'ثبت سفارش انجام نشد.');
      setOrder(body as CreatedOrder);
      setCart({});
      setCustomerName('');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'ارتباط با سرور برقرار نشد.');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div dir="rtl" className="app-shell">
      <header className="topbar">
        <a className="brand" href="#home" aria-label="Aurelia home">
          <span className="brand-mark"><UtensilsCrossed size={18} /></span>
          <span><strong>AURELIA</strong><small>RESTAURANT EXPERIENCE</small></span>
        </a>
        <nav aria-label="ناوبری اصلی">
          <a href="#home">خانه</a>
          <a href="#menu">منو</a>
          <a href="#order">ثبت سفارش</a>
        </nav>
        <a className="cart-link" href="#order">
          <ShoppingBag size={17} />
          <span>سبد سفارش</span>
          <b>{formatAmount(itemCount)}</b>
        </a>
      </header>

      <main>
        <section id="home" className="hero">
          <div className="hero-copy">
            <span className="eyebrow"><span /> تجربه‌ای فراتر از یک وعده غذا</span>
            <h1>طعمِ ماندگار،<br /><em>لحظه‌های ناب.</em></h1>
            <p>روایتی تازه از مهمان‌نوازی؛ جایی که جزئیات، کیفیت و طعم در کنار هم معنا پیدا می‌کنند.</p>
            <a className="primary-link" href="#menu">مشاهده منو <ArrowLeft size={17} /></a>
            <div className="hero-note"><span>01 / 03</span><span className="note-line" /><span>THE ART OF DINING</span></div>
          </div>
          <div className="hero-visual">
            <div className="visual-frame"><Scene /></div>
            <div className="visual-caption"><span>CRAFTED WITH INTENTION</span><span>EST. AURELIA</span></div>
          </div>
          <a className="scroll-cue" href="#menu"><ArrowDown size={15} /> اسکرول برای کشف منو</a>
        </section>

        <section id="menu" className="menu-section">
          <div className="section-heading">
            <div><span className="eyebrow">A CURATED SELECTION</span><h2>منوی <em>آئورلیا</em></h2></div>
            <p>انتخاب‌هایی با تمرکز بر کیفیت مواد اولیه و تعادل طعم‌ها.</p>
          </div>

          <div className="category-tabs" aria-label="فیلتر دسته‌بندی منو">
            <button className={activeCategory === 'all' ? 'active' : ''} onClick={() => setActiveCategory('all')}>همه</button>
            {categories.map((category) => (
              <button key={category.id} className={activeCategory === category.id ? 'active' : ''}
                onClick={() => setActiveCategory(category.id)}>{category.name}</button>
            ))}
          </div>

          {loading ? (
            <div className="state-message"><LoaderCircle className="spin" /> در حال دریافت منو...</div>
          ) : error && !items.length ? (
            <div className="state-message error-state">{error}<small>از روشن بودن سرویس‌های Docker و API اطمینان پیدا کنید.</small></div>
          ) : visibleItems.length === 0 ? (
            <div className="state-message"><ChefHat size={24} /> هنوز آیتمی در این دسته ثبت نشده است.</div>
          ) : (
            <div className="menu-grid">
              {visibleItems.map((item, index) => (
                <article className="menu-card" key={item.id}>
                  <div className="dish-art"><span>{String(index + 1).padStart(2, '0')}</span><UtensilsCrossed size={25} /></div>
                  <div className="dish-meta"><span>{item.category_name ?? 'پیشنهاد سرآشپز'}</span><span> AURELIA</span></div>
                  <h3>{item.name}</h3>
                  <p>{item.description || 'با دقت انتخاب‌شده، با ظرافت آماده‌شده.'}</p>
                  <div className="dish-bottom">
                    <strong>{formatAmount(item.price_minor)} <small>واحد خرد پول</small></strong>
                    <button aria-label={`افزودن ${item.name} به سفارش`} onClick={() => changeQuantity(item.id, 1)}><Plus size={17} /></button>
                  </div>
                </article>
              ))}
            </div>
          )}
          <p className="currency-note">قیمت‌ها به‌صورت عدد صحیح در واحد خرد پول ذخیره می‌شوند؛ واحد پول نهایی هنوز در تنظیمات کسب‌وکار تعریف نشده است.</p>
        </section>

        <section id="order" className="order-section">
          <div className="section-heading">
            <div><span className="eyebrow">YOUR TABLE, YOUR MOMENT</span><h2>ثبت <em>سفارش</em></h2></div>
            <p>اقلام انتخابی را بررسی و درخواست خود را ثبت کنید.</p>
          </div>
          <div className="order-layout">
            <div className="order-lines">
              {cartLines.length === 0 ? (
                <div className="empty-cart"><ShoppingBag size={26} /><span>سبد سفارش شما خالی است.</span><a href="#menu">رفتن به منو</a></div>
              ) : cartLines.map((item) => (
                <div className="order-line" key={item.id}>
                  <div><strong>{item.name}</strong><small>{formatAmount(item.price_minor)} × {formatAmount(item.quantity)}</small></div>
                  <div className="quantity-control">
                    <button onClick={() => changeQuantity(item.id, -1)} aria-label="کاهش تعداد"><Minus size={14} /></button>
                    <b>{formatAmount(item.quantity)}</b>
                    <button onClick={() => changeQuantity(item.id, 1)} aria-label="افزایش تعداد"><Plus size={14} /></button>
                  </div>
                </div>
              ))}
              <div className="total-line"><span>جمع کل</span><strong>{formatAmount(total)}</strong></div>
            </div>
            <form className="order-form" onSubmit={submitOrder}>
              <label htmlFor="customer-name">نام ثبت‌کننده سفارش</label>
              <input id="customer-name" value={customerName} onChange={(event) => setCustomerName(event.target.value)}
                maxLength={120} placeholder="نام شما" required />
              <button className="submit-order" type="submit" disabled={submitting || itemCount === 0}>
                {submitting ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />}
                {submitting ? 'در حال ثبت...' : 'ثبت درخواست سفارش'}
              </button>
              <small className="form-disclaimer">این نسخه صرفاً درخواست سفارش را ثبت می‌کند؛ پرداخت آنلاین و تأیید نهایی هنوز فعال نیست.</small>
              {error && <p className="form-error" role="alert">{error}</p>}
              {order && <div className="success-message" role="status"><Check size={18} /><span>درخواست شما ثبت شد.<b>{order.order_number}</b></span></div>}
            </form>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <a className="brand" href="#home"><span className="brand-mark"><UtensilsCrossed size={18} /></span><strong>AURELIA</strong></a>
        <span>هنر مهمان‌نوازی در جزئیات است.</span>
        <small>© {new Date().getFullYear()} AURELIA RESTAURANT</small>
      </footer>
    </div>
  );
}
