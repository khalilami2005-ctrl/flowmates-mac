export function getFriendlyAuthError(error) {
  const message = String(error?.message || error || '').toLowerCase();

  if (message.includes('invalid login credentials') || message.includes('invalid email or password')) {
    return 'Invalid email or password. Check your license credentials.';
  }

  if (message.includes('email not confirmed')) {
    return 'Please confirm your email before signing in.';
  }

  if (message.includes('no active individual subscription')) {
    return 'This account does not have an active Individual license.';
  }

  if (message.includes('no active') && message.includes('subscription')) {
    return 'No active Individual or Team license found for this account.';
  }

  if (message.includes('cloud login failed')) {
    return 'Cloud login is temporarily unavailable. Check your connection and try again.';
  }

  return 'Login failed. Please try again or contact support.';
}
