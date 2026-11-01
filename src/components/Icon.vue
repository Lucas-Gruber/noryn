<script setup lang="ts">
import {
  Play,
  Clock,
  Square,
  Cpu,
  MemoryStick,
  Activity,
  Anchor,
  Maximize2,
  Minimize2,
  PanelLeft,
  ChevronLeft,
  ChevronsRight,
  ChevronsLeft,
  FolderTree,
  FileText,
  Keyboard,
  EllipsisVertical,
  Server,
  ChevronRight,
  ChevronDown,
  Plus,
  Search,
  Star,
  KeyRound,
  Users,
  LockKeyhole,
  Settings2,
  Terminal,
  Files,
  ArrowUpRight,
  ArrowUp,
  ArrowDown,
  RefreshCw,
  MoreHorizontal,
  X,
  ShieldCheck,
  Check,
  Copy,
  LayoutGrid,
  List,
  CircleHelp,
  Globe,
  Monitor,
  Database,
  HardDrive,
  Command,
  Pencil,
  Trash2,
  Upload,
  Download,
  ExternalLink,
  Link,
  CheckCheck,
  ChevronsUpDown,
  PanelLeftClose,
  CheckCircle2,
  ArrowRight,
  Home,
  Eye,
  ShieldAlert,
  LogOut,
  Grip,
  LoaderCircle,
  FoldVertical,
  UnfoldVertical,
} from 'lucide-vue-next';
defineProps<{ name: string; size?: number }>();
const icons: Record<string, unknown> = {
  Play,
  Clock,
  Square,
  Cpu,
  MemoryStick,
  Activity,
  Anchor,
  Maximize2,
  Minimize2,
  PanelLeft,
  ChevronLeft,
  ChevronsRight,
  ChevronsLeft,
  FolderTree,
  FileText,
  Keyboard,
  EllipsisVertical,
  Server,
  ChevronRight,
  ChevronDown,
  Plus,
  Search,
  Star,
  KeyRound,
  Users,
  LockKeyhole,
  Settings2,
  Terminal,
  Files,
  ArrowUpRight,
  ArrowUp,
  ArrowDown,
  RefreshCw,
  MoreHorizontal,
  X,
  ShieldCheck,
  Check,
  Copy,
  LayoutGrid,
  List,
  CircleHelp,
  Globe,
  Monitor,
  Database,
  HardDrive,
  Command,
  Pencil,
  Trash2,
  Upload,
  Download,
  ExternalLink,
  Link,
  CheckCheck,
  ChevronsUpDown,
  PanelLeftClose,
  CheckCircle2,
  ArrowRight,
  Home,
  Eye,
  ShieldAlert,
  LogOut,
  Grip,
  LoaderCircle,
  FoldVertical,
  UnfoldVertical,
};
</script>
<template>
  <svg
    v-if="['Folder', 'FolderOpen', 'FolderPlus'].includes(name)"
    :width="size || 18"
    :height="size || 18"
    viewBox="0 0 24 24"
    fill="none"
    aria-hidden="true"
    class="folder-symbol"
  >
    <path
      d="M3 7.25A2.75 2.75 0 0 1 5.75 4.5h3.1c.7 0 1.37.28 1.86.77L12.45 7H18.5a2.5 2.5 0 0 1 2.5 2.5V17a2.5 2.5 0 0 1-2.5 2.5H5.5A2.5 2.5 0 0 1 3 17V7.25Z"
      fill="currentColor"
      opacity=".28"
    />
    <path
      v-if="name === 'FolderOpen'"
      d="M5.1 9.5h14.6a1.8 1.8 0 0 1 1.77 2.14l-1.03 5.4a3 3 0 0 1-2.95 2.46H5.5A2.5 2.5 0 0 1 3.04 17.45l-1-5.1A2.4 2.4 0 0 1 4.4 9.5h.7Z"
      fill="currentColor"
      opacity=".85"
    />
    <rect v-else x="3" y="9.5" width="18" height="10" rx="2.5" fill="currentColor" opacity=".85" />
    <path
      v-if="name === 'FolderPlus'"
      d="M12 12v5m-2.5-2.5h5"
      stroke="var(--panel)"
      stroke-width="1.8"
      stroke-linecap="round"
    />
  </svg>
  <component
    v-else
    :is="icons[name] || Server"
    :size="size || 18"
    :stroke-width="1.65"
    aria-hidden="true"
  />
</template>
