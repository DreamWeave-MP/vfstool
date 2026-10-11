--!strict
--!native
-- SPDX-License-Identifier: MIT OR Apache-2.0
-- The `vfstool` command target: its arguments go to the library's parser, whose exit code is the
-- command's status.
local vfstool = require '@vfstool'

return vfstool.run({ ... })
