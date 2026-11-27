import { FormEvent, useEffect, useState } from 'react';
import { ChefHat, LoaderCircle, Plus, RefreshCw, Save, Trash2 } from 'lucide-react';

type MenuCategory = { id: number; restaurant_id: number; restaurant_name: string; name: string; slug: string };
type MenuItem = {
  id: number; restaurant_id: number; restaurant_name: string; category_id: number | null;
  category_name: string | null; name: string; description: string; price_minor: number; available: boolean;
};
type ItemForm = { name: string; description: string; category_id: string; price: string; available: boolean };

const formatAmount = (amount: number) => new Intl.NumberFormat('fa-IR', { maximumFractionDigits: 0 }).format(amount);
const initialItemForm: ItemForm = { name: '', description: '', category_id: '', price: '', available: true };

async function parseResponse<T>(response: Response, fallback: string): Promise<T> {
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error((body as { error?: string }).error || fallback);
  return body as T;
}

export default function RestaurantMenuManager() {
  const [categories, setCategories] = useState<MenuCategory[]>([]);
  const [items, setItems] = useState<MenuItem[]>([]);
  const [categoryName, setCategoryName] = useState('');
  const [itemForm, setItemForm] = useState<ItemForm>(initialItemForm);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editForm, setEditForm] = useState<ItemForm>(initialItemForm);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  async function loadMenu() {
    setLoading(true);
    setError('');
    try {
      const [categoryResponse, itemResponse] = await Promise.all([
        fetch('/api/restaurant/menu/categories', { credentials: 'same-origin', cache: 'no-store' }),
        fetch('/api/restaurant/menu/items', { credentials: 'same-origin', cache: 'no-store' }),
      ]);
      const categoryData = await parseResponse<MenuCategory[]>(categoryResponse, 'دریافت دسته‌ها ناموفق بود.');
      const itemData = await parseResponse<MenuItem[]>(itemResponse, 'دریافت منو ناموفق بود.');
      setCategories(categoryData);
      setItems(itemData);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'خطای ارتباط با سرور');
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void loadMenu(); }, []);

  const toForm = (item: MenuItem): ItemForm => ({
    name: item.name,
    description: item.description,
    category_id: item.category_id ? String(item.category_id) : '',
    price: String(item.price_minor),
    available: item.available,
  });

  async function createCategory(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaving(true); setError(''); setNotice('');
    try {
      const response = await fetch('/api/restaurant/menu/categories', {
        method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: categoryName.trim() }),
      });
      const category = await parseResponse<MenuCategory>(response, 'ثبت دسته ناموفق بود.');
      setCategories((current) => [...current, category].sort((a, b) => a.name.localeCompare(b.name)));
      setCategoryName('');
      setNotice('دسته‌بندی اضافه شد.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'ثبت دسته ناموفق بود.');
    } finally { setSaving(false); }
  }

  async function createItem(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const price = Number(itemForm.price);
    if (!Number.isSafeInteger(price) || price <= 0) { setError('قیمت باید یک عدد صحیح مثبت باشد.'); return; }
    setSaving(true); setError(''); setNotice('');
    try {
      const response = await fetch('/api/restaurant/menu/items', {
        method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: itemForm.name.trim(), description: itemForm.description.trim(),
          category_id: itemForm.category_id ? Number(itemForm.category_id) : null,
          price_minor: price, available: itemForm.available,
        }),
      });
      const item = await parseResponse<MenuItem>(response, 'افزودن غذا ناموفق بود.');
      setItems((current) => [...current, item].sort((a, b) => a.id - b.id));
      setItemForm(initialItemForm);
      setNotice('غذا به منو اضافه شد.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'افزودن غذا ناموفق بود.');
    } finally { setSaving(false); }
  }

  async function saveItem(itemId: number) {
    const price = Number(editForm.price);
    if (!Number.isSafeInteger(price) || price <= 0) { setError('قیمت باید یک عدد صحیح مثبت باشد.'); return; }
    setSaving(true); setError(''); setNotice('');
    try {
      const response = await fetch('/api/restaurant/menu/items/' + itemId, {
        method: 'PUT', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: editForm.name.trim(), description: editForm.description.trim(),
          category_id: editForm.category_id ? Number(editForm.category_id) : null,
          price_minor: price, available: editForm.available,
        }),
      });
      const updated = await parseResponse<MenuItem>(response, 'ذخیره تغییرات ناموفق بود.');
      setItems((current) => current.map((item) => item.id === itemId ? updated : item));
      setEditingId(null);
      setNotice('تغییرات ذخیره شد.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'ذخیره تغییرات ناموفق بود.');
    } finally { setSaving(false); }
  }

  async function toggleAvailability(item: MenuItem) {
    setError(''); setNotice('');
    try {
      const response = await fetch('/api/restaurant/menu/items/' + item.id + '/availability', {
        method: 'PATCH', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ available: !item.available }),
      });
      const updated = await parseResponse<MenuItem>(response, 'تغییر وضعیت ناموفق بود.');
      setItems((current) => current.map((row) => row.id === item.id ? updated : row));
      setNotice(updated.available ? 'غذا دوباره در دسترس قرار گرفت.' : 'غذا موقتاً ناموجود شد.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'تغییر وضعیت ناموفق بود.');
    }
  }

  async function softDeleteItem(item: MenuItem) {
    if (!window.confirm('این غذا از منو حذف و ناموجود می‌شود؛ تاریخچه سفارش‌ها حفظ می‌شود. ادامه؟')) return;
    setError(''); setNotice('');
    try {
      const response = await fetch('/api/restaurant/menu/items/' + item.id, {
        method: 'DELETE', credentials: 'same-origin',
      });
      if (!response.ok) throw await parseResponse<unknown>(response, 'حذف غذا ناموفق بود.');
      setItems((current) => current.map((row) => row.id === item.id ? { ...row, available: false } : row));
      setNotice('غذا از فروش خارج شد؛ سوابق سفارش محفوظ است.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'حذف غذا ناموفق بود.');
    }
  }

  async function deleteCategory(category: MenuCategory) {
    if (!window.confirm('دسته حذف شود؟ غذاها حذف نمی‌شوند و بدون دسته باقی می‌مانند.')) return;
    setError(''); setNotice('');
    try {
      const response = await fetch('/api/restaurant/menu/categories/' + category.id, {
        method: 'DELETE', credentials: 'same-origin',
      });
      if (!response.ok) throw await parseResponse<unknown>(response, 'حذف دسته ناموفق بود.');
      setCategories((current) => current.filter((row) => row.id !== category.id));
      setItems((current) => current.map((item) => item.category_id === category.id
        ? { ...item, category_id: null, category_name: null } : item));
      setNotice('دسته حذف شد؛ غذاها حفظ شدند.');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'حذف دسته ناموفق بود.');
    }
  }

  return (
    <section className="restaurant-menu-manager" aria-labelledby="menu-manager-title">
      <div className="staff-section-title">
        <div><ChefHat size={19} /><h2 id="menu-manager-title">مدیریت منو</h2></div>
        <button className="staff-ghost-button" onClick={() => void loadMenu()} disabled={loading}><RefreshCw size={15} /> تازه‌سازی</button>
      </div>
      {error && <p className="staff-feedback staff-error" role="alert">{error}</p>}
      {notice && <p className="staff-feedback staff-success" role="status">{notice}</p>}
      {loading ? <div className="staff-state"><LoaderCircle className="spin" /> در حال دریافت منو...</div> : <>
        <div className="menu-manager-grid">
          <section className="menu-manager-panel">
            <h3>دسته‌بندی‌ها</h3>
            <form className="menu-category-form" onSubmit={createCategory}>
              <input value={categoryName} onChange={(event) => setCategoryName(event.target.value)} maxLength={80} placeholder="نام دسته جدید" required />
              <button type="submit" disabled={saving}><Plus size={15} /> افزودن</button>
            </form>
            {categories.length === 0 ? <p className="menu-manager-empty">هنوز دسته‌ای ندارید.</p> : <div className="menu-category-list">
              {categories.map((category) => <div key={category.id} className="menu-category-row"><span>{category.name}</span><button onClick={() => void deleteCategory(category)} aria-label={"حذف دسته " + category.name}><Trash2 size={14} /></button></div>)}
            </div>}
          </section>
          <section className="menu-manager-panel">
            <h3>افزودن غذای جدید</h3>
            <form className="menu-item-form" onSubmit={createItem}>
              <label>نام غذا<input value={itemForm.name} onChange={(event) => setItemForm((current) => ({ ...current, name: event.target.value }))} maxLength={160} required /></label>
              <label>دسته<select value={itemForm.category_id} onChange={(event) => setItemForm((current) => ({ ...current, category_id: event.target.value }))}><option value="">بدون دسته</option>{categories.map((category) => <option value={category.id} key={category.id}>{category.name}</option>)}</select></label>
              <label>قیمت (تومان، فرض اولیه)<input type="number" min={1} max={100000000} step={1} inputMode="numeric" value={itemForm.price} onChange={(event) => setItemForm((current) => ({ ...current, price: event.target.value }))} required /></label>
              <label>توضیحات<textarea value={itemForm.description} onChange={(event) => setItemForm((current) => ({ ...current, description: event.target.value }))} maxLength={1000} rows={2} /></label>
              <label className="menu-available-check"><input type="checkbox" checked={itemForm.available} onChange={(event) => setItemForm((current) => ({ ...current, available: event.target.checked }))} /> در دسترس برای سفارش</label>
              <button className="menu-save-button" type="submit" disabled={saving}><Plus size={16} /> افزودن به منو</button>
            </form>
          </section>
        </div>

        <div className="menu-items-manager">
          <div className="menu-items-heading"><h3>غذاهای منو</h3><span>{formatAmount(items.length)} مورد</span></div>
          {items.length === 0 ? <div className="staff-state">هنوز غذایی ثبت نشده است.</div> : <div className="menu-managed-list">
            {items.map((item) => <article className={"menu-managed-item" + (item.available ? '' : ' unavailable')} key={item.id}>
              {editingId === item.id ? (
                <form className="menu-edit-form" onSubmit={(event) => { event.preventDefault(); void saveItem(item.id); }}>
                  <label>نام<input value={editForm.name} onChange={(event) => setEditForm((current) => ({ ...current, name: event.target.value }))} maxLength={160} required /></label>
                  <label>دسته<select value={editForm.category_id} onChange={(event) => setEditForm((current) => ({ ...current, category_id: event.target.value }))}><option value="">بدون دسته</option>{categories.map((category) => <option value={category.id} key={category.id}>{category.name}</option>)}</select></label>
                  <label>قیمت (تومان)<input type="number" min={1} max={100000000} step={1} value={editForm.price} onChange={(event) => setEditForm((current) => ({ ...current, price: event.target.value }))} required /></label>
                  <label>توضیحات<textarea value={editForm.description} onChange={(event) => setEditForm((current) => ({ ...current, description: event.target.value }))} maxLength={1000} rows={2} /></label>
                  <label className="menu-available-check"><input type="checkbox" checked={editForm.available} onChange={(event) => setEditForm((current) => ({ ...current, available: event.target.checked }))} /> موجود</label>
                  <div className="menu-edit-actions"><button type="submit" disabled={saving}><Save size={15} /> ذخیره</button><button type="button" onClick={() => setEditingId(null)}>انصراف</button></div>
                </form>
              ) : <>
                <div className="menu-managed-info"><span className="menu-managed-category">{item.category_name || 'بدون دسته'}</span><h4>{item.name}</h4><p>{item.description || 'بدون توضیح'}</p><strong>{formatAmount(item.price_minor)} <small>تومان*</small></strong></div>
                <div className="menu-managed-actions">
                  <button className={item.available ? 'menu-status-on' : 'menu-status-off'} onClick={() => void toggleAvailability(item)}>{item.available ? 'موجود' : 'ناموجود'}</button>
                  <button onClick={() => { setEditingId(item.id); setEditForm(toForm(item)); }}><Save size={14} /> ویرایش</button>
                  <button className="menu-delete-action" onClick={() => void softDeleteItem(item)} disabled={!item.available}><Trash2 size={14} /> حذف</button>
                </div>
              </>}
            </article>)}
          </div>}
        </div>
      </>}
    </section>
  );
}
