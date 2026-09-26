export type Provider = "modrinth" | "curseforge";
export type ContentType = "mod" | "resourcepack" | "shader" | "datapack";

export interface ModItem {
  id: string;
  slug: string;
  name: string;
  author?: string | null;
  description?: string;
  iconUrl?: string | null;
  provider: Provider;
  contentType: ContentType;
  downloads?: number | null;
  installed?: boolean;
  updateAvailable?: boolean;
  categories?: string[];
}

export interface ModFilter {
  query: string;
  provider: Provider | "both";
  contentType: ContentType;
  sort: "relevance" | "downloads" | "date";
  categories: string[];
  gameVersion?: string | null;
  loader?: string | null;
}
