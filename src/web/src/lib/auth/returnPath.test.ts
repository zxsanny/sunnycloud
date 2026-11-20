import { describe, expect, it } from 'vitest';
import { isFollowableUrl, safeReturnPath } from './returnPath';

describe('safeReturnPath', () => {
  it.each([
    'https://evil.test',
    '//evil.test',
    '/\\evil.test',
    'javascript:alert(1)',
    '/a\\b',
    'files/x',
  ])('sends %s to the start page', (hostile) => {
    // Act
    const path = safeReturnPath(hostile);

    // Assert
    expect(path).toBe('/');
  });

  it.each([null, undefined, ''])('sends %s to the start page', (missing) => {
    // Act
    const path = safeReturnPath(missing);

    // Assert
    expect(path).toBe('/');
  });

  it('refuses a path with a control character', () => {
    // Act
    const path = safeReturnPath('/files\n/x');

    // Assert
    expect(path).toBe('/');
  });

  it.each(['/', '/files/x', '/files?folder=a%20b', '/s/abc#frag'])(
    'keeps the relative path %s',
    (ok) => {
      // Act
      const path = safeReturnPath(ok);

      // Assert
      expect(path).toBe(ok);
    },
  );
});

describe('isFollowableUrl', () => {
  it.each(['https://accounts.google.com/o/oauth2/v2/auth?x=1', 'http://localhost:9000/auth'])(
    'follows %s',
    (url) => {
      // Act
      const result = isFollowableUrl(url);

      // Assert
      expect(result).toBe(true);
    },
  );

  it.each(['javascript:alert(1)', 'data:text/html,x', 'not a url', '/relative'])(
    'refuses %s',
    (url) => {
      // Act
      const result = isFollowableUrl(url);

      // Assert
      expect(result).toBe(false);
    },
  );
});
