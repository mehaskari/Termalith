import { useMemo, useState } from "react";
import {
  type LucideIcon,
  Activity,
  AirVent,
  Anchor,
  Antenna,
  AppWindow,
  Archive,
  Atom,
  Award,
  Battery,
  BatteryCharging,
  Beaker,
  Bluetooth,
  Bookmark,
  Bot,
  Box,
  Boxes,
  Braces,
  BrickWall,
  BrickWallFire,
  Briefcase,
  BriefcaseBusiness,
  Bug,
  Building,
  Building2,
  Cable,
  Calendar,
  Camera,
  Car,
  Cast,
  Castle,
  Cctv,
  ChartBar,
  ChartLine,
  ChartPie,
  Circle,
  CircleAlert,
  CircleCheck,
  CircleDot,
  CircleHelp,
  CircleX,
  Clock,
  Cloud,
  CloudCog,
  CloudDownload,
  CloudOff,
  CloudUpload,
  Code,
  CodeXml,
  Cog,
  Compass,
  Computer,
  Contact,
  Container,
  Cpu,
  Crown,
  Database,
  DatabaseBackup,
  DatabaseZap,
  Diamond,
  Earth,
  EthernetPort,
  Eye,
  EyeOff,
  Factory,
  Fan,
  File,
  FileCode,
  FileText,
  Files,
  Fingerprint,
  Flag,
  Flame,
  FlaskConical,
  Folder,
  FolderGit2,
  FolderLock,
  FolderOpen,
  Gamepad2,
  Gauge,
  Gem,
  GitBranch,
  GitMerge,
  Globe,
  Hammer,
  HardDrive,
  HardDriveDownload,
  HardDriveUpload,
  Headset,
  Heart,
  Hexagon,
  Hospital,
  House,
  Inbox,
  Key,
  KeyRound,
  KeySquare,
  Keyboard,
  Landmark,
  Laptop,
  LaptopMinimal,
  Layers,
  Leaf,
  Lightbulb,
  Link,
  Lock,
  LockKeyhole,
  Mail,
  Map,
  MapPin,
  MemoryStick,
  MessageSquare,
  Mic,
  Microscope,
  Monitor,
  MonitorCog,
  MonitorDot,
  MonitorSmartphone,
  Moon,
  Mountain,
  Mouse,
  Network,
  Octagon,
  Package,
  PanelsTopLeft,
  PcCase,
  Pentagon,
  Phone,
  PhoneCall,
  Plane,
  Plug,
  PlugZap,
  Printer,
  Projector,
  Radio,
  RadioTower,
  Rocket,
  Router,
  Rss,
  Satellite,
  SatelliteDish,
  ScanFace,
  School,
  Send,
  Server,
  ServerCog,
  ServerCrash,
  ServerOff,
  Settings,
  Share2,
  Shield,
  ShieldAlert,
  ShieldBan,
  ShieldCheck,
  ShieldHalf,
  Ship,
  Signal,
  Siren,
  SlidersHorizontal,
  Smartphone,
  Snowflake,
  Speaker,
  Square,
  SquareTerminal,
  Star,
  Store,
  Sun,
  Tablet,
  Tag,
  Telescope,
  Tent,
  Terminal,
  TestTube,
  Thermometer,
  TreePine,
  Triangle,
  Tv,
  Unlock,
  Unplug,
  User,
  UserCog,
  Users,
  Warehouse,
  Waypoints,
  Webcam,
  Webhook,
  Wifi,
  WifiOff,
  Workflow,
  Wrench,
  Zap,
} from "lucide-react";
import type { SessionIcon } from "../terminal/api";

/**
 * Pictures for sessions and folders: a curated part of Lucide (ISC
 * licence, no brand logos), grouped for the picker. Stored by a slug such
 * as "server-cog"; names saved by 0.5.9 map through ALIASES.
 */
const GROUPS: Array<{ label: string; icons: Array<[string, LucideIcon]> }> = [
  { label: "Servers", icons: [["Server", Server], ["ServerCog", ServerCog], ["ServerCrash", ServerCrash], ["ServerOff", ServerOff], ["Database", Database], ["DatabaseBackup", DatabaseBackup], ["DatabaseZap", DatabaseZap], ["HardDrive", HardDrive], ["HardDriveDownload", HardDriveDownload], ["HardDriveUpload", HardDriveUpload], ["Cpu", Cpu], ["MemoryStick", MemoryStick], ["Container", Container], ["Box", Box], ["Boxes", Boxes], ["Package", Package], ["Layers", Layers], ["Cloud", Cloud], ["CloudCog", CloudCog], ["CloudUpload", CloudUpload], ["CloudDownload", CloudDownload], ["CloudOff", CloudOff]] },
  { label: "Computers", icons: [["Monitor", Monitor], ["MonitorSmartphone", MonitorSmartphone], ["MonitorCog", MonitorCog], ["MonitorDot", MonitorDot], ["Laptop", Laptop], ["LaptopMinimal", LaptopMinimal], ["Computer", Computer], ["PcCase", PcCase], ["Tablet", Tablet], ["Smartphone", Smartphone], ["AppWindow", AppWindow], ["PanelsTopLeft", PanelsTopLeft], ["Terminal", Terminal], ["SquareTerminal", SquareTerminal], ["Keyboard", Keyboard], ["Mouse", Mouse], ["Tv", Tv], ["Gamepad2", Gamepad2]] },
  { label: "Network", icons: [["Router", Router], ["Network", Network], ["Wifi", Wifi], ["WifiOff", WifiOff], ["Radio", Radio], ["RadioTower", RadioTower], ["Antenna", Antenna], ["Satellite", Satellite], ["SatelliteDish", SatelliteDish], ["Cable", Cable], ["EthernetPort", EthernetPort], ["Plug", Plug], ["PlugZap", PlugZap], ["Unplug", Unplug], ["Globe", Globe], ["Earth", Earth], ["Signal", Signal], ["Waypoints", Waypoints], ["Workflow", Workflow], ["Share2", Share2], ["Rss", Rss], ["Bluetooth", Bluetooth], ["Cast", Cast], ["Webhook", Webhook], ["Link", Link]] },
  { label: "Security", icons: [["Shield", Shield], ["ShieldCheck", ShieldCheck], ["ShieldAlert", ShieldAlert], ["ShieldHalf", ShieldHalf], ["ShieldBan", ShieldBan], ["BrickWall", BrickWall], ["BrickWallFire", BrickWallFire], ["Lock", Lock], ["LockKeyhole", LockKeyhole], ["Unlock", Unlock], ["Key", Key], ["KeyRound", KeyRound], ["KeySquare", KeySquare], ["Fingerprint", Fingerprint], ["ScanFace", ScanFace], ["Eye", Eye], ["EyeOff", EyeOff], ["Bug", Bug], ["Siren", Siren]] },
  { label: "Devices", icons: [["Printer", Printer], ["Cctv", Cctv], ["Camera", Camera], ["Webcam", Webcam], ["Phone", Phone], ["PhoneCall", PhoneCall], ["Headset", Headset], ["Speaker", Speaker], ["Mic", Mic], ["Projector", Projector], ["Thermometer", Thermometer], ["Gauge", Gauge], ["Battery", Battery], ["BatteryCharging", BatteryCharging], ["Zap", Zap], ["Lightbulb", Lightbulb], ["Fan", Fan], ["AirVent", AirVent], ["Car", Car], ["Bot", Bot]] },
  { label: "Places", icons: [["House", House], ["Building", Building], ["Building2", Building2], ["Factory", Factory], ["Warehouse", Warehouse], ["Store", Store], ["School", School], ["Hospital", Hospital], ["Landmark", Landmark], ["Castle", Castle], ["Tent", Tent], ["MapPin", MapPin], ["Map", Map], ["Flag", Flag], ["Briefcase", Briefcase], ["BriefcaseBusiness", BriefcaseBusiness], ["Users", Users], ["User", User], ["UserCog", UserCog], ["Contact", Contact]] },
  { label: "Files and apps", icons: [["Folder", Folder], ["FolderOpen", FolderOpen], ["FolderGit2", FolderGit2], ["FolderLock", FolderLock], ["File", File], ["FileCode", FileCode], ["FileText", FileText], ["Files", Files], ["Archive", Archive], ["Mail", Mail], ["Inbox", Inbox], ["Send", Send], ["MessageSquare", MessageSquare], ["Calendar", Calendar], ["Clock", Clock], ["Code", Code], ["CodeXml", CodeXml], ["Braces", Braces], ["GitBranch", GitBranch], ["GitMerge", GitMerge]] },
  { label: "Tools", icons: [["Wrench", Wrench], ["Hammer", Hammer], ["Settings", Settings], ["Cog", Cog], ["SlidersHorizontal", SlidersHorizontal], ["Activity", Activity], ["ChartLine", ChartLine], ["ChartBar", ChartBar], ["ChartPie", ChartPie], ["TestTube", TestTube], ["FlaskConical", FlaskConical], ["Microscope", Microscope], ["Beaker", Beaker], ["Rocket", Rocket], ["Plane", Plane], ["Ship", Ship], ["Anchor", Anchor], ["Compass", Compass], ["Telescope", Telescope], ["Atom", Atom]] },
  { label: "Shapes and marks", icons: [["Star", Star], ["Heart", Heart], ["Circle", Circle], ["Square", Square], ["Triangle", Triangle], ["Hexagon", Hexagon], ["Octagon", Octagon], ["Diamond", Diamond], ["Pentagon", Pentagon], ["CircleDot", CircleDot], ["CircleCheck", CircleCheck], ["CircleAlert", CircleAlert], ["CircleX", CircleX], ["CircleHelp", CircleHelp], ["Bookmark", Bookmark], ["Tag", Tag], ["Award", Award], ["Crown", Crown], ["Gem", Gem], ["Flame", Flame], ["Snowflake", Snowflake], ["Sun", Sun], ["Moon", Moon], ["Leaf", Leaf], ["TreePine", TreePine], ["Mountain", Mountain]] },
];

/** "ServerCog" → "server-cog", "Building2" → "building-2". */
const slugOf = (name: string) =>
  name
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .replace(/([a-zA-Z])([0-9])/g, "$1-$2")
    .toLowerCase();
/** "server-cog" → "Server cog". */
const labelOf = (slug: string) => {
  const words = slug.replace(/-/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
};

interface Picture {
  slug: string;
  label: string;
  Glyph: LucideIcon;
}

const PICTURES = new globalThis.Map<string, Picture>();
export const ICON_GROUPS: Array<{ label: string; pictures: Picture[] }> = GROUPS.map((g) => ({
  label: g.label,
  pictures: g.icons.map(([name, Glyph]) => {
    const slug = slugOf(name);
    const p = { slug, label: labelOf(slug), Glyph };
    PICTURES.set(slug, p);
    return p;
  }),
}));

/** Names used by the first icon set (0.5.9). */
const ALIASES: Record<string, string> = {
  desktop: "monitor",
  window: "app-window",
  vm: "monitor-cog",
  switch: "network",
  firewall: "brick-wall",
  storage: "hard-drive",
  web: "globe",
  camera: "cctv",
  phone: "smartphone",
  chip: "cpu",
  home: "house",
  office: "building-2",
  lab: "flask-conical",
};

const picture = (name: string): Picture | undefined => PICTURES.get(ALIASES[name] ?? name);

export const ICON_NAMES = [...PICTURES.keys()];

/** Colors offered for icons (readable on light and dark backgrounds). */
export const ICON_COLORS: Array<{ name: string; value: string }> = [
  { name: "Gray", value: "#868e96" },
  { name: "Red", value: "#e03131" },
  { name: "Orange", value: "#f08c00" },
  { name: "Yellow", value: "#d4a106" },
  { name: "Lime", value: "#66a80f" },
  { name: "Green", value: "#2f9e44" },
  { name: "Teal", value: "#0c8599" },
  { name: "Cyan", value: "#1098ad" },
  { name: "Blue", value: "#1971c2" },
  { name: "Indigo", value: "#4263eb" },
  { name: "Violet", value: "#7048e8" },
  { name: "Pink", value: "#c2255c" },
  { name: "Brown", value: "#8d6e63" },
];

export function iconLabel(name: string): string {
  return picture(name)?.label ?? "Icon";
}

/** A session or folder icon; unknown names show the server picture. */
export function SessionIconGlyph({ icon, size = 16, className }: { icon: SessionIcon; size?: number; className?: string }) {
  const p = picture(icon.name) ?? picture("server");
  if (!p) return null;
  const { Glyph } = p;
  return (
    <Glyph
      className={`ic session-icon ${className ?? ""}`}
      size={size}
      color={icon.color ?? "currentColor"}
      strokeWidth={1.75}
      aria-hidden="true"
      focusable="false"
    />
  );
}

/** Picks a picture (with search) and a color; "Automatic" (null) shows the protocol or folder. */
export function IconPicker({
  value,
  onChange,
  automaticLabel = "Automatic",
}: {
  value: SessionIcon | null;
  onChange: (icon: SessionIcon | null) => void;
  automaticLabel?: string;
}) {
  const color = value?.color ?? null;
  const [query, setQuery] = useState("");
  const chosen = value ? (ALIASES[value.name] ?? value.name) : null;
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return ICON_GROUPS;
    return ICON_GROUPS.map((g) => ({
      label: g.label,
      pictures: g.pictures.filter((p) => p.label.toLowerCase().includes(q) || g.label.toLowerCase().includes(q)),
    })).filter((g) => g.pictures.length > 0);
  }, [query]);
  return (
    <div className="icon-picker">
      <div className="icon-picker-head">
        <button
          type="button"
          role="radio"
          aria-checked={value === null}
          className="icon-choice auto"
          onClick={() => onChange(null)}
        >
          {automaticLabel}
        </button>
        <input
          className="icon-search"
          value={query}
          placeholder={`Search ${ICON_NAMES.length} icons…`}
          aria-label="Search icons"
          spellCheck={false}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div className="icon-groups">
        {groups.length === 0 && <p className="hint">No icon matches.</p>}
        {groups.map((g) => (
          <section key={g.label} aria-label={g.label}>
            <h4>{g.label}</h4>
            <div className="icon-grid" role="radiogroup" aria-label={g.label}>
              {g.pictures.map((p) => (
                <button
                  key={p.slug}
                  type="button"
                  role="radio"
                  aria-checked={chosen === p.slug}
                  aria-label={p.label}
                  title={p.label}
                  className="icon-choice"
                  onClick={() => onChange({ name: p.slug, color })}
                >
                  <SessionIconGlyph icon={{ name: p.slug, color }} size={18} />
                </button>
              ))}
            </div>
          </section>
        ))}
      </div>
      {value && (
        <div className="icon-colors" role="radiogroup" aria-label="Icon color">
          <button
            type="button"
            role="radio"
            aria-checked={color === null}
            className="color-choice none"
            title="Default color"
            aria-label="Default color"
            onClick={() => onChange({ name: value.name, color: null })}
          />
          {ICON_COLORS.map((c) => (
            <button
              key={c.value}
              type="button"
              role="radio"
              aria-checked={color === c.value}
              className="color-choice"
              title={c.name}
              aria-label={c.name}
              style={{ background: c.value }}
              onClick={() => onChange({ name: value.name, color: c.value })}
            />
          ))}
        </div>
      )}
    </div>
  );
}
