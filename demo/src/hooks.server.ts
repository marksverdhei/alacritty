import type { Handle } from '@sveltejs/kit';

/**
 * Set Cross-Origin headers required by WebContainers API.
 * These headers enable SharedArrayBuffer which WebContainers need.
 */
export const handle: Handle = async ({ event, resolve }) => {
	const response = await resolve(event);

	response.headers.set('Cross-Origin-Embedder-Policy', 'require-corp');
	response.headers.set('Cross-Origin-Opener-Policy', 'same-origin');

	return response;
};
