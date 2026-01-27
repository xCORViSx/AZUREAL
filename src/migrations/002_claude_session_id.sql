-- Add claude_session_id column to sessions table
-- Stores the Claude CLI session ID for --resume functionality

ALTER TABLE sessions ADD COLUMN claude_session_id TEXT;
