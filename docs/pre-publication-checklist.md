# Pre-publication checklist

## Commit history identity

The current history contains 11 commits authored and committed as `StackForge Maintainer <maintainers@stackforge.dev>` and one commit from the Captain Code identity. Future commits in this clone are configured as `Captain Code <amiralizolfaghary1387@gmail.com>`.

Because the repository is still private and has no public release, normalize the obsolete author and committer identity before making it public. This rewrites commit IDs, so do not do it without a recoverable mirror backup and a maintenance window. The safest procedure is:

```bash
# From a separate directory, create an immutable rollback copy.
git clone --mirror git@github.com:captaincode-tech/scafflare.git scafflare-backup.git

# Clone a working copy and rewrite only the obsolete identity fields.
git clone git@github.com:captaincode-tech/scafflare.git scafflare-history-normalized
cd scafflare-history-normalized

git filter-repo --force --commit-callback '
if commit.author_email == b"maintainers@stackforge.dev":
    commit.author_name = b"Captain Code"
    commit.author_email = b"amiralizolfaghary1387@gmail.com"
if commit.committer_email == b"maintainers@stackforge.dev":
    commit.committer_name = b"Captain Code"
    commit.committer_email = b"amiralizolfaghary1387@gmail.com"
'

# Inspect the rewritten result before replacing the private remote history.
git log --format="%h %an <%ae> | %cn <%ce> %s"
git push --force --mirror origin
```

Only perform the force push after confirming that no collaborators have unpublished work based on the existing history. Preserve the mirror backup until the rewritten remote has been independently verified.

## GitHub settings before public visibility

Enable **Private Vulnerability Reporting** in **Settings → Advanced Security** immediately after the repository becomes public, then confirm that the **Report a vulnerability** button appears on the Security Advisories page. Watch the repository with **Security alerts** notifications enabled.

Recommended repository description:

> Composable Rust CLI for generating Node.js and TypeScript backends from safe YAML recipes.

Recommended topics: `rust`, `cli`, `scaffolding`, `code-generation`, `nodejs`, `typescript`, `yaml`, `developer-tools`.

## First release

After history and repository settings are confirmed, create and push the signed tag `v0.1.0`. The release workflow verifies Rust, recipes, fixture generation, production npm audits, and RustSec advisories before it creates the GitHub Release and uploads checksummed native archives.
