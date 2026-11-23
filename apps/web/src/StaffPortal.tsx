import { FormEvent, useEffect, useState } from 'react';
import { ArrowLeft, ChefHat, ClipboardList, History, LoaderCircle, LogOut, RefreshCw, ShieldCheck, Store, UserRound } from 'lucide-react';

type StaffRole = 'owner' | 'manager' | 'cashier' | 'chef' | 'waiter';
type AuditRole = StaffRole | 'restaurant_owner';
type StaffIdentity = { id: number; username: string; role: StaffRole };
type PendingRestaurantApplication = {
  id: number;
  restaurant_id: number;
  restaurant_name: string;
  owner_username: string;
  contact_name: string;
  contact_phone: string;
  contact_email: string;
  city: string;
  address: string;
  cuisine: string;
  description: string;
  submitted_at: string;
};
type OrderSummary = {
  id: number;
  order_number: string;
  customer_name: string;
  status: string;
  total_minor: number;
};
type AuditEvent = {
  id: number;
  event_type: string;
  actor_staff_user_id: number | null;
  actor_username: string | null;
  subject_staff_user_id: number | null;
  subject_username: string | null;
  attempted_username: string | null;
  subject_role: AuditRole | null;
  created_at: string;
};
type AuditPage = { events: AuditEvent[]; next_cursor: number | null };

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

const auditEventLabels: Record<string, string> = {
  owner_bootstrapped: 'ایجاد حساب مالک',
  staff_provisioned_cli: 'ایجاد حساب کارمند',
  staff_login_succeeded: 'ورود موفق',
  staff_login_failed: 'ورود ناموفق',
  staff_logout: 'خروج از حساب',
  restaurant_application_submitted: 'ثبت درخواست رستوران',
  restaurant_application_approved: 'تأیید رستوران',
  restaurant_application_rejected: 'رد درخواست رستوران',
};

const formatAuditTime = (value: string) => {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('fa-IR', { dateStyle: 'medium', timeStyle: 'short' }).format(date);
};

function isAuditPage(value: unknown): value is AuditPage {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Record<string, unknown>;
  if (!Array.isArray(candidate.events)) return false;
  if (candidate.next_cursor !== null && typeof candidate.next_cursor !== 'number') return false;
  return candidate.events.every((entry) => {
    if (!entry || typeof entry !== 'object') return false;
    const event = entry as Record<string, unknown>;
    return typeof event.id === 'number'
      && typeof event.event_type === 'string'
      && (event.actor_staff_user_id === null || typeof event.actor_staff_user_id === 'number')
      && (event.actor_username === null || typeof event.actor_username === 'string')
      && (event.subject_staff_user_id === null || typeof event.subject_staff_user_id === 'number')
      && (event.subject_username === null || typeof event.subject_username === 'string')
      && (event.attempted_username === null || typeof event.attempted_username === 'string')
      && (event.subject_role === null || ['owner', 'manager', 'cashier', 'chef', 'waiter', 'restaurant_owner'].includes(String(event.subject_role)))
      && typeof event.created_at === 'string';
  });
}

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
  const [auditEvents, setAuditEvents] = useState<AuditEvent[]>([]);
  const [auditCursor, setAuditCursor] = useState<number | null>(null);
  const [auditLoading, setAuditLoading] = useState(false);
  const [auditError, setAuditError] = useState('');
  const [auditRefreshKey, setAuditRefreshKey] = useState(0);
  const [applications, setApplications] = useState<PendingRestaurantApplication[]>([]);
  const [applicationsLoading, setApplicationsLoading] = useState(false);
  const [applicationsError, setApplicationsError] = useState('');
  const [reviewingApplicationId, setReviewingApplicationId] = useState<number | null>(null);
  const [applicationsRefreshKey, setApplicationsRefreshKey] = useState(0);

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

  useEffect(() => {
    if (identity?.role !== 'owner') {
      setAuditEvents([]);
      setAuditCursor(null);
      setAuditError('');
      return;
    }
    let cancelled = false;
    setAuditLoading(true);
    setAuditError('');
    fetch('/api/admin/audit?limit=50', { credentials: 'same-origin', cache: 'no-store' })
      .then(async (response) => {
        if (!response.ok) throw await responseError(response, 'دریافت سوابق امنیتی ناموفق بود.');
        const body: unknown = await response.json();
        if (!isAuditPage(body)) throw new Error('ساختار سوابق امنیتی معتبر نیست.');
        return body;
      })
      .then((page) => {
        if (!cancelled) {
          setAuditEvents(page.events);
          setAuditCursor(page.next_cursor);
        }
      })
      .catch((reason: unknown) => {
        if (!cancelled) setAuditError(reason instanceof Error ? reason.message : 'دریافت سوابق ناموفق بود.');
      })
      .finally(() => {
        if (!cancelled) setAuditLoading(false);
      });
    return () => { cancelled = true; };
  }, [identity, auditRefreshKey]);

  useEffect(() => {
    if (identity?.role !== 'owner') {
      setApplications([]);
      setApplicationsError('');
      return;
    }
    let cancelled = false;
    setApplicationsLoading(true);
    setApplicationsError('');
    fetch('/api/admin/restaurants/applications', { credentials: 'same-origin', cache: 'no-store' })
      .then(async (response) => {
        if (!response.ok) throw await responseError(response, 'دریافت درخواست‌های رستوران ناموفق بود.');
        const body: unknown = await response.json();
        if (!Array.isArray(body)) throw new Error('ساختار درخواست‌های رستوران معتبر نیست.');
        return body as PendingRestaurantApplication[];
      })
      .then((data) => { if (!cancelled) setApplications(data); })
      .catch((reason: unknown) => {
        if (!cancelled) setApplicationsError(reason instanceof Error ? reason.message : 'دریافت درخواست‌ها ناموفق بود.');
      })
      .finally(() => { if (!cancelled) setApplicationsLoading(false); });
    return () => { cancelled = true; };
  }, [identity, applicationsRefreshKey]);

  async function reviewApplication(applicationId: number, decision: 'approve' | 'reject') {
    if (decision === 'reject' && !window.confirm('این درخواست رد شود؟ حساب مالک غیرفعال باقی می‌ماند.')) return;
    setReviewingApplicationId(applicationId);
    setApplicationsError('');
    setError('');
    try {
      const response = await fetch('/api/admin/restaurants/applications/' + applicationId + '/review', {
        method: 'POST',
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ decision, note: '' }),
      });
      if (!response.ok) throw await responseError(response, 'بررسی درخواست انجام نشد.');
      setApplications((current) => current.filter((application) => application.id !== applicationId));
      setNotice(decision === 'approve' ? 'رستوران تأیید و حساب مالک فعال شد.' : 'درخواست رد شد.');
      setAuditRefreshKey((key) => key + 1);
    } catch (reason) {
      setApplicationsError(reason instanceof Error ? reason.message : 'بررسی درخواست ناموفق بود.');
    } finally {
      setReviewingApplicationId(null);
    }
  }

  async function loadMoreAudit() {
    if (identity?.role !== 'owner' || auditCursor === null || auditLoading) return;
    setAuditLoading(true);
    setAuditError('');
    try {
      const response = await fetch(`/api/admin/audit?limit=50&before_id=${auditCursor}`, {
        credentials: 'same-origin',
        cache: 'no-store',
      });
      if (!response.ok) throw await responseError(response, 'دریافت صفحه بعد ناموفق بود.');
      const body: unknown = await response.json();
      if (!isAuditPage(body)) throw new Error('ساختار سوابق امنیتی معتبر نیست.');
      setAuditEvents((current) => {
        const knownIds = new Set(current.map((event) => event.id));
        return [...current, ...body.events.filter((event) => !knownIds.has(event.id))];
      });
      setAuditCursor(body.next_cursor);
    } catch (reason) {
      setAuditError(reason instanceof Error ? reason.message : 'دریافت صفحه بعد ناموفق بود.');
    } finally {
      setAuditLoading(false);
    }
  }

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
            {identity.role === 'owner' && (
              <>
                <section className="staff-applications-section" aria-labelledby="restaurant-applications-title">
                  <div className="staff-section-title">
                    <div><Store size={19} /><h2 id="restaurant-applications-title">درخواست‌های ثبت رستوران</h2></div>
                    <button className="staff-ghost-button" onClick={() => setApplicationsRefreshKey((key) => key + 1)} disabled={applicationsLoading}>
                      <RefreshCw size={15} /> تازه‌سازی
                    </button>
                  </div>
                  {applicationsError && <p className="staff-feedback staff-error" role="alert">{applicationsError}</p>}
                  {applicationsLoading && applications.length === 0 ? (
                    <div className="staff-state"><LoaderCircle className="spin" /> در حال دریافت درخواست‌ها...</div>
                  ) : applications.length === 0 && !applicationsError ? (
                    <div className="staff-state"><Store size={22} /> درخواست جدیدی وجود ندارد.</div>
                  ) : (
                    <div className="staff-application-list">
                      {applications.map((application) => (
                        <article className="staff-application-card" key={application.id}>
                          <div className="staff-application-head">
                            <div><span className="staff-application-id">APP / {formatAmount(application.id)}</span><h3>{application.restaurant_name}</h3></div>
                            <span className="staff-status status-pending">در انتظار بررسی</span>
                          </div>
                          <div className="staff-application-details">
                            <span>مالک: <b>{application.contact_name}</b></span>
                            <span>نام کاربری: <b>{application.owner_username}</b></span>
                            <span>تلفن: <b>{application.contact_phone}</b></span>
                            <span>ایمیل: <b>{application.contact_email}</b></span>
                            <span>شهر: <b>{application.city}</b></span>
                            <span>نوع غذا: <b>{application.cuisine}</b></span>
                            <span className="application-address">آدرس: <b>{application.address}</b></span>
                          </div>
                          {application.description && <p className="staff-application-description">{application.description}</p>}
                          <div className="staff-application-actions">
                            <button className="application-approve" onClick={() => reviewApplication(application.id, 'approve')} disabled={reviewingApplicationId !== null}>
                              {reviewingApplicationId === application.id ? <LoaderCircle className="spin" size={15} /> : <ShieldCheck size={15} />}
                              تأیید و فعال‌سازی
                            </button>
                            <button className="application-reject" onClick={() => reviewApplication(application.id, 'reject')} disabled={reviewingApplicationId !== null}>
                              رد درخواست
                            </button>
                          </div>
                        </article>
                      ))}
                    </div>
                  )}
                </section>
                <section className="staff-audit-section" aria-labelledby="staff-audit-title">
                <div className="staff-section-title">
                  <div><History size={19} /><h2 id="staff-audit-title">سوابق امنیتی</h2></div>
                  <button className="staff-ghost-button" onClick={() => setAuditRefreshKey((key) => key + 1)} disabled={auditLoading}>
                    <RefreshCw size={15} /> تازه‌سازی
                  </button>
                </div>
                {auditError && <p className="staff-feedback staff-error" role="alert">{auditError}</p>}
                {auditLoading && auditEvents.length === 0 ? (
                  <div className="staff-state"><LoaderCircle className="spin" /> در حال دریافت سوابق...</div>
                ) : auditEvents.length === 0 && !auditError ? (
                  <div className="staff-state">هنوز رویداد امنیتی ثبت نشده است.</div>
                ) : (
                  <div className="staff-audit-list">
                    {auditEvents.map((event) => (
                      <article className="staff-audit-card" key={event.id}>
                        <div className="staff-audit-head">
                          <strong>{auditEventLabels[event.event_type] ?? event.event_type}</strong>
                          <time dateTime={event.created_at}>{formatAuditTime(event.created_at)}</time>
                        </div>
                        <div className="staff-audit-meta">
                          <span>عامل: <b>{event.actor_username ?? (event.event_type === 'staff_login_failed' ? 'بدون نشست' : 'سیستم')}</b></span>
                          <span>{event.event_type === 'staff_login_failed' ? 'نام کاربری تلاش‌شده' : 'حساب هدف'}: <b>{event.subject_username ?? event.attempted_username ?? '—'}</b></span>
                          <span>نقش: <b>{event.subject_role === 'restaurant_owner' ? 'مالک رستوران' : event.subject_role ? roleLabels[event.subject_role] : 'نامشخص'}</b></span>
                          <span>شناسه رویداد: <b>{formatAmount(event.id)}</b></span>
                        </div>
                      </article>
                    ))}
                  </div>
                )}
                {auditCursor !== null && (
                  <button className="staff-audit-more" onClick={loadMoreAudit} disabled={auditLoading}>
                    {auditLoading ? <LoaderCircle className="spin" size={16} /> : null}
                    {auditLoading ? 'در حال دریافت...' : 'نمایش رویدادهای قدیمی‌تر'}
                  </button>
                )}
                </section>
              </>
            )}
          </section>
        )}

        {error && <p className="staff-feedback staff-error" role="alert">{error}</p>}
        {notice && <p className="staff-feedback staff-notice" role="status">{notice}</p>}
      </main>
    </div>
  );
}
