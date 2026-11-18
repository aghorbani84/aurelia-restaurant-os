import { FormEvent, useEffect, useState } from 'react';
import { ArrowLeft, ChefHat, ClipboardList, LoaderCircle, LogOut, RefreshCw, ShieldCheck, UserRound } from 'lucide-react';

type StaffRole = 'owner' | 'manager' | 'cashier' | 'chef' | 'waiter';
type StaffIdentity = { id: number; username: string; role: StaffRole };
type OrderSummary = {
  id: number;
  order_number: string;
  customer_name: string;
  status: string;
  total_minor: number;
};

const roleLabels: Record<StaffRole, string> = {
  owner: 'مالک',
  manager: 'مدیر',
  cashier: 'صندوق‌دار',
  chef: 'آشپز',
  waiter: 'گارسون',
};

const orderEndpoints: Record<StaffRole, string> = {
  owner: '/api/admin/orders',
  manager: '/api/admin/orders',
  cashier: '/api/admin/orders',
  chef: '/api/kitchen/orders',
  waiter: '/api/service/orders',
};

const statusLabels: Record<string, string> = {
  pending: 'در انتظار',
  accepted: 'پذیرفته‌شده',
  preparing: 'در حال آماده‌سازی',
  ready: 'آماده تحویل',
  completed: 'تکمیل‌شده',
  cancelled: 'لغوشده',
};

const formatAmount = (value: number) =>
  new Intl.NumberFormat('fa-IR', { maximumFractionDigits: 0 }).format(value);

function isStaffIdentity(value: unknown): value is StaffIdentity {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate.id === 'number'
    && typeof candidate.username === 'string'
    && ['owner', 'manager', 'cashier', 'chef', 'waiter'].includes(String(candidate.role));
}

async function responseError(response: Response, fallback: string): Promise<Error> {
  const body = await response.json().catch(() => ({})) as { error?: string };
  return new Error(body.error || fallback);
}

export default function StaffPortal() {
  const [identity, setIdentity] = useState<StaffIdentity | null>(null);
  const [sessionLoading, setSessionLoading] = useState(true);
  const [orders, setOrders] = useState<OrderSummary[]>([]);
  const [ordersLoading, setOrdersLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [refreshKey, setRefreshKey] = useState(0);

  useEffect(() => {
    let cancelled = false;
    fetch('/api/auth/me', { credentials: 'same-origin', cache: 'no-store' })
      .then(async (response) => {
        if (response.status === 401) return null;
        if (!response.ok) throw await responseError(response, 'بررسی نشست ناموفق بود.');
        const body: unknown = await response.json();
        if (!isStaffIdentity(body)) throw new Error('پاسخ نشست از سرور معتبر نیست.');
        return body;
      })
      .then((staff) => {
        if (!cancelled) setIdentity(staff);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : 'ارتباط با سرور برقرار نشد.');
      })
      .finally(() => {
        if (!cancelled) setSessionLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!identity) {
      setOrders([]);
      return;
    }
    let cancelled = false;
    setOrdersLoading(true);
    setError('');
    fetch(orderEndpoints[identity.role], { credentials: 'same-origin', cache: 'no-store' })
      .then(async (response) => {
        if (!response.ok) throw await responseError(response, 'دریافت سفارش‌ها ناموفق بود.');
        const body: unknown = await response.json();
        if (!Array.isArray(body)) throw new Error('ساختار فهرست سفارش‌ها معتبر نیست.');
        return body as OrderSummary[];
      })
      .then((data) => {
        if (!cancelled) setOrders(data);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : 'دریافت سفارش‌ها ناموفق بود.');
      })
      .finally(() => {
        if (!cancelled) setOrdersLoading(false);
      });
    return () => { cancelled = true; };
  }, [identity, refreshKey]);

  async function login(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError('');
    setNotice('');
    setSubmitting(true);
    try {
      const response = await fetch('/api/auth/login', {
        method: 'POST',
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: username.trim(), password }),
      });
      if (!response.ok) throw await responseError(response, 'ورود انجام نشد.');
      const body: unknown = await response.json();
      if (!isStaffIdentity(body)) throw new Error('پاسخ ورود از سرور معتبر نیست.');
      setIdentity(body);
      setPassword('');
      setNotice('ورود با موفقیت انجام شد.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'ارتباط با سرور برقرار نشد.');
    } finally {
      setSubmitting(false);
    }
  }

  async function logout() {
    setError('');
    setNotice('');
    setSubmitting(true);
    try {
      const response = await fetch('/api/auth/logout', {
        method: 'POST',
        credentials: 'same-origin',
        cache: 'no-store',
      });
      if (!response.ok) throw await responseError(response, 'خروج از حساب انجام نشد.');
      setIdentity(null);
      setOrders([]);
      setNotice('از حساب خارج شدید.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'خروج از حساب ناموفق بود.');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div dir="rtl" className="staff-shell">
      <header className="staff-topbar">
        <a className="brand" href="/" aria-label="بازگشت به فروشگاه AURELIA">
          <span className="brand-mark"><ChefHat size={18} /></span>
          <span><strong>AURELIA</strong><small>STAFF WORKSPACE</small></span>
        </a>
        <a className="staff-back-link" href="/"><ArrowLeft size={15} /> بازگشت به فروشگاه</a>
      </header>

      <main className="staff-main">
        <div className="staff-heading">
          <span className="eyebrow">SECURE STAFF AREA</span>
          <h1>فضای کاری <em>کارکنان</em></h1>
          <p>دسترسی به اطلاعات سفارش‌ها بر اساس نقش ثبت‌شده حساب شما.</p>
        </div>

        {sessionLoading ? (
          <section className="staff-panel staff-loading"><LoaderCircle className="spin" /> در حال بررسی نشست...</section>
        ) : !identity ? (
          <section className="staff-panel staff-login-panel">
            <div className="staff-panel-icon"><ShieldCheck size={23} /></div>
            <h2>ورود کارکنان</h2>
            <p>حساب کاربری باید توسط مالک رستوران ایجاد شده باشد.</p>
            <form className="staff-login-form" onSubmit={login}>
              <label htmlFor="staff-username">نام کاربری</label>
              <input id="staff-username" value={username} onChange={(event) => setUsername(event.target.value)}
                autoComplete="username" maxLength={32} required placeholder="نام کاربری کارکنان" />
              <label htmlFor="staff-password">گذرواژه</label>
              <input id="staff-password" type="password" value={password}
                onChange={(event) => setPassword(event.target.value)} autoComplete="current-password"
                maxLength={128} required placeholder="گذرواژه حساب" />
              <button className="staff-primary-button" type="submit" disabled={submitting}>
                {submitting ? <LoaderCircle className="spin" size={17} /> : <ShieldCheck size={17} />}
                {submitting ? 'در حال ورود...' : 'ورود امن'}
              </button>
            </form>
          </section>
        ) : (
          <section className="staff-workspace">
            <div className="staff-account-bar">
              <div className="staff-account">
                <span className="staff-avatar"><UserRound size={19} /></span>
                <span><strong>{identity.username}</strong><small>{roleLabels[identity.role]}</small></span>
              </div>
              <div className="staff-actions">
                <button className="staff-ghost-button" onClick={() => setRefreshKey((key) => key + 1)} disabled={ordersLoading}>
                  <RefreshCw size={15} /> تازه‌سازی
                </button>
                <button className="staff-ghost-button" onClick={logout} disabled={submitting}>
                  <LogOut size={15} /> خروج
                </button>
              </div>
            </div>

            <div className="staff-section-title">
              <div><ClipboardList size={19} /><h2>سفارش‌های قابل مشاهده</h2></div>
              <span>فقط مشاهده</span>
            </div>

            {ordersLoading ? (
              <div className="staff-state"><LoaderCircle className="spin" /> در حال دریافت سفارش‌ها...</div>
            ) : orders.length === 0 ? (
              <div className="staff-state"><ClipboardList size={24} /> سفارشی برای نمایش وجود ندارد.</div>
            ) : (
              <div className="staff-order-grid">
                {orders.map((order) => (
                  <article className="staff-order-card" key={order.id}>
                    <div className="staff-order-top">
                      <strong>{order.order_number}</strong>
                      <span className={`staff-status status-${order.status}`}>{statusLabels[order.status] ?? order.status}</span>
                    </div>
                    <h3>{order.customer_name}</h3>
                    <div className="staff-order-total"><span>مبلغ ثبت‌شده</span><strong>{formatAmount(order.total_minor)} <small>واحد خرد</small></strong></div>
                  </article>
                ))}
              </div>
            )}
            <p className="staff-readonly-note">این پنل در این مرحله فقط سفارش‌ها را نمایش می‌دهد؛ تغییر وضعیت، مدیریت کارکنان و عملیات مالی هنوز فعال نیست.</p>
          </section>
        )}

        {error && <p className="staff-feedback staff-error" role="alert">{error}</p>}
        {notice && <p className="staff-feedback staff-notice" role="status">{notice}</p>}
      </main>
    </div>
  );
}
