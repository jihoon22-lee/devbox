export interface DistroInfo {
  name: string;
  version: number;
  default: boolean;
}

export interface ContainerInfo {
  id: string;
  name: string;
  image: string;
  status: string;
  ports: string;
}

export interface GitStatus {
  path: string;
  branch: string;
  changes: number;
  clean: boolean;
}
