export interface RouteDef<C> {
  /** An exact path. Dynamic segments (`/s/:token`) are added with the first page that needs one. */
  path: string;
  component: C;
  /** Reachable without a session; the app-level 401 handling leaves these routes alone. */
  public?: boolean;
}

/** A trailing slash does not change the route. */
function normalise(path: string): string {
  return path.length > 1 && path.endsWith('/') ? path.slice(0, -1) : path;
}

export function matchRoute<C>(routes: RouteDef<C>[], path: string): RouteDef<C> | null {
  const wanted = normalise(path);
  return routes.find((route) => route.path === wanted) ?? null;
}
