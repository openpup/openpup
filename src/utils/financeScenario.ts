import type { FinanceSkillKey } from '../stores/scenarioStore';

export interface FinanceScenarioPreparedInput {
  input: string;
  forcedPup: string | null;
  matchedSkill: FinanceSkillKey | null;
}

export function prepareFinanceScenarioInput(
  rawInput: string,
  selectedPupKey: string,
): FinanceScenarioPreparedInput {
  const explicitForcedPup = selectedPupKey !== 'alpha' ? selectedPupKey : null;
  return {
    input: rawInput,
    forcedPup: explicitForcedPup,
    matchedSkill: null,
  };
}
