// Single source of truth for the Minecraft Usage Guidelines disclaimer
// and the project repo URL. Shared by the About panel and its tests so
// the verbatim text cannot drift.
//
// Reference: https://www.minecraft.net/en-us/usage-guidelines
// Essential guidelines section ("Prominently include the disclaimer...").

export const DISCLAIMER_TEXT =
  'NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.';

export const REPO_URL = 'https://github.com/AntonBabchenko/Lucerna';

// Pages on the repository the Settings link to (About, Help). All https, all
// opened through `openExternalHttps`. The policy and the licence point at
// `main` — the living documents — not at the installed tag.
export const BUG_REPORT_URL = `${REPO_URL}/issues/new?template=bug_report.md`;
export const SECURITY_POLICY_URL = `${REPO_URL}/security/policy`;
export const PRIVACY_POLICY_URL = `${REPO_URL}/blob/main/PRIVACY.md`;
export const LICENSE_URL = `${REPO_URL}/blob/main/LICENSE`;
