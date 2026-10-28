import {
  ArrowDownWideNarrow,
  ArrowRight,
  ChevronDown,
  CloudDownload,
  Folder,
  FolderOpen,
  GraphStar,
  GripVertical,
  HardDrive,
  History,
  Info,
  Link,
  LockKeyhole,
  Menu,
  Plus,
  Search,
  SlidersHorizontal,
  SquareArrowOutUpRight,
  TriangleAlert,
  X,
  type LucideIcon,
} from 'lucide-react';

export type IconName =
  | 'folder'
  | 'projectFolder'
  | 'packages'
  | 'settings'
  | 'info'
  | 'search'
  | 'plus'
  | 'arrow'
  | 'chevron'
  | 'history'
  | 'download'
  | 'close'
  | 'grip'
  | 'disk'
  | 'warning'
  | 'lock'
  | 'project'
  | 'link'
  | 'sort'
  | 'external'
  | 'menu';

const iconComponents = {
  folder: Folder,
  projectFolder: Folder,
  packages: GraphStar,
  settings: SlidersHorizontal,
  info: Info,
  search: Search,
  plus: Plus,
  arrow: ArrowRight,
  chevron: ChevronDown,
  history: History,
  download: CloudDownload,
  close: X,
  grip: GripVertical,
  disk: HardDrive,
  warning: TriangleAlert,
  lock: LockKeyhole,
  project: FolderOpen,
  link: Link,
  sort: ArrowDownWideNarrow,
  external: SquareArrowOutUpRight,
  menu: Menu,
} satisfies Record<IconName, LucideIcon>;

export function Icon({ name, size = 18, className }: { name: IconName; size?: number; className?: string }) {
  const Component = iconComponents[name];
  return <Component className={className ? `icon ${className}` : 'icon'} size={size} strokeWidth={2} aria-hidden="true" focusable="false" />;
}
