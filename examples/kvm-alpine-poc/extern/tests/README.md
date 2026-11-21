# Instructor test assets

The project configuration expresses the two checks inline so the platform agent can execute them without a host-side shell wrapper. The first invokes bash syntax validation. Only a syntactically valid script is invoked with 17 and 25; expected stdout is exactly 42 followed by a newline, and exit code must be zero.

| Student | Syntax | Execution | Expected assessment |
|---|---|---|---|
| student1 | Pass | Prints 42, exit 0 | 10/10 |
| student2 | Pass | Prints 425, exit 0 | 2/10: syntax criterion only |
| student3 | Fail | Skipped because syntax prerequisite failed | 0/10: syntax criterion fails; no functional assertion is made |

These fixtures are synthetic. Guest boot, agent, transport, timeout, or cleanup failures are infrastructure errors; the engine must leave the grade pending rather than apply the rubric to missing evidence.
