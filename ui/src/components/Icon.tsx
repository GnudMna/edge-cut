// 線画アイコン (24×24 のグリッドに 1.75 px の線で描く)

const PATHS = {
  folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
  paste: "M9 4h6v3H9zM9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2M9 13h6M9 16h4",
  batch: "M4 7l8-4 8 4-8 4zM4 12l8 4 8-4M4 17l8 4 8-4",
  save: "M12 4v11M7 10l5 5 5-5M5 20h14",
  copy: "M9 9h10v10H9zM5 15V5h10",
  close: "M6 6l12 12M18 6L6 18",
  minimize: "M6 12h12",
  maximize: "M6 6h12v12H6z",
  restore: "M8 8h10v10H8zM6 16V6h10",
  menu: "M4 7h16M4 12h16M4 17h16",
  panel: "M4 5h16v14H4zM14 5v14",
  eyedropper: "M15 4l5 5-2 2-1-1-7 7H7v-3l7-7-1-1zM5 19l2-2",
  reset: "M4 12a8 8 0 1 0 2.3-5.6M4 4v4h4",
  link: "M10 14a4 4 0 0 0 5.6 0l3-3a4 4 0 0 0-5.6-5.6l-1 1M14 10a4 4 0 0 0-5.6 0l-3 3a4 4 0 0 0 5.6 5.6l1-1",
  unlink: "M15.6 14l3-3a4 4 0 0 0-5.6-5.6l-1 1M8.4 10l-3 3a4 4 0 0 0 5.6 5.6l1-1M4 4l16 16",
  swap: "M7 4L4 7l3 3M4 7h13M17 14l3 3-3 3M20 17H7",
  sun: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
  moon: "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z",
  monitor: "M3 5h18v11H3zM8 20h8M12 16v4",
  book: "M5 4h10a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3zM5 17a3 3 0 0 1 3-3h10",
  file: "M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6",
  download: "M12 4v11M7 10l5 5 5-5M5 20h14",
  check: "M5 12l5 5 9-10",
  alert: "M12 3l10 18H2zM12 10v5M12 18v.5",
  chevron: "M6 9l6 6 6-6",
  image: "M4 5h16v14H4zM4 16l5-5 4 4 2-2 5 5M15 9.5a1.5 1.5 0 1 0 0 .01",
  zoomIn: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4M8 11h6M11 8v6",
  zoomOut: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4M8 11h6",
  fit: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
  trash: "M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13",
  stop: "M7 7h10v10H7z",
  crop: "M6 2v14a2 2 0 0 0 2 2h14M2 6h14a2 2 0 0 1 2 2v14",
  info: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5M12 8v.5",
  refresh: "M20 12a8 8 0 1 1-2.3-5.6M20 4v4h-4",
} as const;

export type IconName = keyof typeof PATHS;

interface Props {
  name: IconName;
  size?: number;
  className?: string;
}

export function Icon({ name, size = 16, className }: Props) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
