# GitHub Branch Protection Configuration

This document outlines the branch protection rules that need to be configured in the GitHub repository settings to enforce the PR-based development workflow.

## Required Branch Protection Rules for `main` branch

### Access Settings
1. **Navigate to**: Repository Settings → Branches → Add rule
2. **Branch name pattern**: `main`

### Protection Rules to Enable

#### ✅ Restrict pushes that create files larger than 100 MB
- Prevents accidentally committing large files

#### ✅ Require a pull request before merging
- **Required approving reviews**: 1
- **Dismiss stale PR approvals when new commits are pushed**: ✅
- **Require review from code owners**: ❌ (optional, enable if CODEOWNERS file exists)
- **Restrict approvals to users with write permissions**: ✅
- **Allow specified actors to bypass required pull requests**: ❌

#### ✅ Require status checks to pass before merging
- **Require branches to be up to date before merging**: ✅
- **Status checks that are required**:
  - `Check` (from CI workflow)
  - `Test Suite (ubuntu-latest, stable)` (from CI workflow)
  - `Test Suite (windows-latest, stable)` (from CI workflow)  
  - `Test Suite (macos-latest, stable)` (from CI workflow)
  - `Documentation` (from CI workflow)

#### ✅ Require conversation resolution before merging
- Ensures all PR comments are addressed

#### ✅ Require signed commits
- Enhances security by requiring commit signatures

#### ✅ Require linear history
- Prevents merge commits, enforces clean git history
- Use "Squash and merge" or "Rebase and merge" strategies

#### ✅ Require deployments to succeed before merging
- ❌ (disable unless you have deployment environments set up)

#### ✅ Lock branch
- ❌ (keep disabled for normal development)

#### ✅ Do not allow bypassing the above settings
- Ensures rules apply to administrators as well

#### ✅ Restrict pushes that create files larger than specified limit
- Set to reasonable limit (e.g., 100 MB)

## Additional Repository Settings

### General Settings
- **Allow merge commits**: ❌ (disable to enforce linear history)
- **Allow squash merging**: ✅ (recommended)
- **Allow rebase merging**: ✅ (recommended)
- **Always suggest updating pull request branches**: ✅
- **Allow auto-merge**: ✅ (optional, useful for automated PRs)
- **Automatically delete head branches**: ✅ (keeps repository clean)

### Actions Settings
- **Allow GitHub Actions to create and approve pull requests**: ✅ (needed for Release Please)

### GitHub App Configuration ✅ Already Configured

This repository uses the **REPOSITORY_BUTLER** GitHub App for enhanced permissions:

- **App ID**: Stored in `REPOSITORY_BUTLER_APP_ID` secret
- **Private Key**: Stored in `REPOSITORY_BUTLER_PEM` secret
- **Benefits**: Higher rate limits, better security, cleaner attribution

**Benefits of GitHub App**:
- Higher rate limits
- Better security (scoped permissions)
- Cleaner commit attribution
- Bypasses branch protection for automated PRs

## Workflow Integration

With these settings enabled:

1. **All changes must go through PRs**
2. **CI checks must pass** before merging
3. **At least one approval** required
4. **Linear git history** maintained
5. **Release Please** can create automated release PRs
6. **Automated releases** triggered by merging release PRs

## ✅ Configuration Applied

Branch protection rules have been successfully configured using GitHub CLI:

```bash
gh api repos/mpecan/runcept/branches/main/protection --method PUT --input branch-protection.json
```

**Current Settings**:
- ✅ Required status checks: 5 checks configured
- ✅ Required reviews: 1 approval required
- ✅ Dismiss stale reviews: Enabled
- ✅ Enforce for admins: Enabled
- ✅ Linear history: Enforced
- ✅ Conversation resolution: Required
- ✅ Force pushes: Blocked
- ✅ Branch deletions: Blocked

You can view the current settings at: `https://github.com/mpecan/runcept/settings/branches`

## Verification

After configuration, verify the setup by:

1. Attempting to push directly to main (should be blocked)
2. Creating a test PR and ensuring CI runs
3. Verifying that merge is blocked until checks pass
4. Confirming that Release Please can create PRs

## Notes

- These rules will prevent direct pushes to `main`
- All development must happen in feature branches
- Release Please will create automated release PRs
- The existing CI workflow already supports PR-based development