export interface RouteDef<C> {
  /** Static segments and `:name` parameters, e.g. `/folders/:id`. */
  path: string;
  component: C;
  /** Reachable without a session; the app-level 401 handling leaves these routes alone. */
  public?: boolean;
}

export interface RouteMatch<C> {
  route: RouteDef<C>;
  params: Record<string, string>;
}

function segments(path: string): string[] {
  return path.split('/').filter((part) => part.length > 0);
}

/** A trailing slash does not change the route; a malformed percent escape in a parameter matches nothing. */
export function matchRoute<C>(routes: RouteDef<C>[], path: string): RouteMatch<C> | null {
  const actual = segments(path);
  for (const route of routes) {
    const pattern = segments(route.path);
    if (pattern.length !== actual.length) continue;
    const params: Record<string, string> = {};
    let matched = true;
    for (const [at, part] of pattern.entries()) {
      const value = actual[at] ?? '';
      if (!part.startsWith(':')) {
        matched = part === value;
      } else {
        try {
          params[part.slice(1)] = decodeURIComponent(value);
        } catch (cause) {
          if (!(cause instanceof URIError)) throw cause;
          matched = false;
        }
      }
      if (!matched) break;
    }
    if (matched) return { route, params };
  }
  return null;
}
