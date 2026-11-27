import { describe, expect, it } from 'vitest';
import {
  ABSENT,
  DEGRADED,
  EMPTY_SEARCH,
  MODULE_UNAVAILABLE,
  NOT_ENOUGH_EVIDENCE,
  REVISION_CONFLICT,
} from './state-copy';

describe('state copy', () => {
  it('uses the accepted sentences verbatim', () => {
    expect(ABSENT).toBe('No editorial position is published. An instance statement would appear here when published.');
    expect(DEGRADED).toBe('Text is not held. The source address is unmeasured.');
    expect(EMPTY_SEARCH).toBe('No works match these constraints. Remove the last constraint.');
    expect(REVISION_CONFLICT).toBe('Your write was based on an older revision. Choose how to merge.');
    expect(MODULE_UNAVAILABLE).toBe('This operation is unavailable because its module is not enabled.');
    expect(NOT_ENOUGH_EVIDENCE).toBe('There is not enough evidence for a verdict.');
  });
});
