import { afterEach, describe, expect, it } from 'vitest';
import { PUBLIC_SHARE_PATH, removeTokenFromAddress, shareContentUrl, tokenOfPath } from './address';
import { typeLabel } from './typeLabel';

const TOKEN = 'sc_shr_Q7mVb4LpT8wZr1NcY6dHs3EjA5uG0oRkWiPqXe9fK2x';

afterEach(() => history.replaceState(null, '', '/'));

describe('the token in the address', () => {
  it('leaves no token in the address or the history entry after it is removed', () => {
    // Arrange
    history.replaceState({ stale: TOKEN }, '', `/s/${TOKEN}?x=1#${TOKEN}`);
    const entries = history.length;

    // Act
    removeTokenFromAddress();

    // Assert
    expect(location.pathname).toBe(PUBLIC_SHARE_PATH);
    expect(location.href).not.toContain(TOKEN);
    expect(JSON.stringify(history.state)).not.toContain(TOKEN);
    expect(location.search).toBe('');
    expect(location.hash).toBe('');
    expect(history.length).toBe(entries);
  });

  it('reads the token from /s/<token> and from nothing else', () => {
    // Arrange

    // Act
    const found = tokenOfPath(`/s/${TOKEN}`);

    // Assert
    expect(found).toBe(TOKEN);
    expect(tokenOfPath('/s')).toBeNull();
    expect(tokenOfPath('/s/')).toBeNull();
    expect(tokenOfPath(`/s/${TOKEN}/extra`)).toBeNull();
    expect(tokenOfPath('/folders/abc')).toBeNull();
  });

  it('builds the content URL with the token encoded as one path segment', () => {
    // Arrange
    const odd = 'a/b?c';

    // Act
    const url = shareContentUrl(odd);

    // Assert
    expect(url).toBe('/api/v1/public-shares/a%2Fb%3Fc/content');
  });
});

describe('the type line', () => {
  it('names the known types and shows an unknown type as the type itself', () => {
    // Arrange

    // Act
    const labels = [
      typeLabel('image/jpeg'),
      typeLabel('application/pdf; x=1'),
      typeLabel('text/x-odd'),
      typeLabel(null),
    ];

    // Assert
    expect(labels).toEqual(['JPEG image', 'PDF document', 'text/x-odd', 'File']);
  });
});
