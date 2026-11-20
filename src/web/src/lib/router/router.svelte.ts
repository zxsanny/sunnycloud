import { matchRoute, type RouteDef } from './match';

class RouteState {
  /** The table the Router renders; set once by it, so parameters can be read from any page. */
  table: RouteDef<unknown>[] = [];
  path = $state(globalThis.location.pathname);
  search = $state(globalThis.location.search);
  hash = $state(globalThis.location.hash);

  /** Parameters of the matched route, such as `id` of `/folders/:id`. */
  get params(): Record<string, string> {
    return matchRoute(this.table, this.path)?.params ?? {};
  }

  get query(): URLSearchParams {
    return new URLSearchParams(this.search);
  }

  /** Path and query of the page, the value a sign-in carries as its return path. */
  get relative(): string {
    return this.path + this.search;
  }

  read() {
    this.path = globalThis.location.pathname;
    this.search = globalThis.location.search;
    this.hash = globalThis.location.hash;
  }
}

export const route = new RouteState();

export function navigate(to: string, options: { replace?: boolean } = {}) {
  if (options.replace) {
    history.replaceState(null, '', to);
  } else {
    history.pushState(null, '', to);
  }
  route.read();
}

/** Same-origin page links navigate inside the app; API links, downloads and new-tab links stay with the browser. */
function onClick(event: MouseEvent) {
  if (event.defaultPrevented || event.button !== 0) return;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  if (!(event.target instanceof Element)) return;
  const link = event.target.closest('a');
  if (link === null || link.target !== '' || link.hasAttribute('download')) return;
  const url = new URL(link.href, globalThis.location.href);
  if (url.origin !== globalThis.location.origin || url.pathname.startsWith('/api/')) return;
  event.preventDefault();
  navigate(url.pathname + url.search + url.hash);
}

/** Starts listening for back/forward and link clicks; the returned function stops it. */
export function startRouter(): () => void {
  const onPop = () => route.read();
  globalThis.addEventListener('popstate', onPop);
  document.addEventListener('click', onClick);
  return () => {
    globalThis.removeEventListener('popstate', onPop);
    document.removeEventListener('click', onClick);
  };
}

/** Leaves the app for the provider's sign-in page; the only navigation to another origin the app makes. */
export function followExternal(url: string) {
  globalThis.location.assign(url);
}
