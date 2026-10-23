import { reactive } from 'vue';
import { fail } from './useNoryn';
export interface MenuItem {
  label: string;
  icon?: string;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  separator?: boolean;
  action: () => unknown | Promise<unknown>;
}
export const menu = reactive({ open: false, x: 0, y: 0, items: [] as MenuItem[] });
export function showMenu(event: MouseEvent, items: MenuItem[]) {
  event.preventDefault();
  event.stopPropagation();
  Object.assign(menu, { open: true, x: event.clientX, y: event.clientY, items });
}
export function closeMenu() {
  menu.open = false;
}
export async function choose(item: MenuItem) {
  if (item.disabled) return;
  closeMenu();
  try {
    await item.action();
  } catch (error) {
    fail(error);
  }
}
