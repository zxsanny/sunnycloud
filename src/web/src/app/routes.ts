import type { PageComponent, RouteDef } from '$lib/router';
import { matchRoute } from '$lib/router';
import HomePage from '../pages/HomePage.svelte';
import NotFoundPage from '../pages/NotFoundPage.svelte';
import SetupPage from '../pages/SetupPage.svelte';
import SignInPage from '../pages/SignInPage.svelte';

/** `/invite#<token>` is the sign-in page in its invite state. */
export const routes: RouteDef<PageComponent>[] = [
  { path: '/', component: HomePage },
  { path: '/login', component: SignInPage, public: true },
  { path: '/invite', component: SignInPage, public: true },
  { path: '/setup', component: SetupPage, public: true },
];

export const notFound: PageComponent = NotFoundPage;

export function isPublicPath(path: string): boolean {
  return matchRoute(routes, path)?.public === true;
}
