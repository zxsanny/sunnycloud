import { describe, expect, it } from 'vitest';
import { matchRoute, type RouteDef } from './match';

const routes: RouteDef<string>[] = [
  { path: '/', component: 'home' },
  { path: '/login', component: 'login', public: true },
  { path: '/folders/:id', component: 'folder' },
];

describe('matchRoute', () => {
  it('finds the root route', () => {
    // Act
    const found = matchRoute(routes, '/');

    // Assert
    expect(found?.route.component).toBe('home');
  });

  it('finds a route and keeps its public flag', () => {
    // Act
    const found = matchRoute(routes, '/login');

    // Assert
    expect(found?.route).toMatchObject({ component: 'login', public: true });
  });

  it('ignores a trailing slash', () => {
    // Act
    const found = matchRoute(routes, '/login/');

    // Assert
    expect(found?.route.component).toBe('login');
  });

  it('answers null for an unknown path so the not-found page shows', () => {
    // Act
    const found = matchRoute(routes, '/nowhere');

    // Assert
    expect(found).toBeNull();
  });

  it('does not match a prefix of a route', () => {
    // Act
    const found = matchRoute(routes, '/login/extra');

    // Assert
    expect(found).toBeNull();
  });

  it('captures a parameter and decodes it', () => {
    // Act
    const found = matchRoute(routes, '/folders/a%20b');

    // Assert
    expect(found?.route.component).toBe('folder');
    expect(found?.params).toEqual({ id: 'a b' });
  });

  it('matches nothing for a malformed percent escape in a parameter', () => {
    // Act
    const found = matchRoute(routes, '/folders/%E0%A4%A');

    // Assert
    expect(found).toBeNull();
  });
});
