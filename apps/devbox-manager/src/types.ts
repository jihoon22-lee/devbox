export interface Asset {
  name: string;
  url: string;
}

export interface LatestRelease {
  tag: string;
  published_at: string;
  assets: Asset[];
}

export interface InstalledApp {
  app: string;
  version: string;
  mode: "portable" | "installer";
  exe_path: string;
}
