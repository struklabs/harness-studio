import { initialParams, isUnsetParam, type LaunchParamsForm } from './params';

/** Fields visible in the Simple launch form. All other form fields are advanced. */
const SIMPLE_FIELDS = new Set<keyof LaunchParamsForm>(['context_size']);

/** Count hidden non-default choices without changing the form or the baseline.
 * Pass paramsFromPreset(null, model, settings) for model/application defaults.
 * Catalog entries are explicit overrides: false and zero count, unset values do not.
 * A typed API key and its keychain reference represent one control.
 */
export function countHiddenParams(params: LaunchParamsForm, defaults = initialParams()): number {
  let count = 0;
  for (const key of Object.keys(defaults) as (keyof LaunchParamsForm)[]) {
    if (SIMPLE_FIELDS.has(key) || key === 'catalog_params' || key === 'api_key' || key === 'api_key_ref') continue;
    const value = params[key];
    const baseline = defaults[key];
    if (isUnsetParam(value) && isUnsetParam(baseline)) continue;
    if (!Object.is(value, baseline)) count++;
  }
  if (params.api_key !== defaults.api_key || params.api_key_ref !== defaults.api_key_ref) count++;
  return count + Object.values(params.catalog_params).filter(value => !isUnsetParam(value)).length;
}
