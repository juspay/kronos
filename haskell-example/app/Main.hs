{-# LANGUAGE OverloadedStrings #-}

-- | Sample Haskell application demonstrating the Kronos SDK.
--
-- This example mirrors the TypeScript CLI test (test-immediate.ts) and shows
-- the full lifecycle of a Kronos job:
--
--   1. Build a KronosServiceClient
--   2. Create an HTTP endpoint pointing at the mock server
--   3. Schedule an IMMEDIATE job targeting that endpoint
--   4. Poll ListJobExecutions until the job reaches a terminal state
--   5. Print the final execution status
--   6. Clean up: cancel the job and delete the endpoint
module Main where

import Control.Concurrent (threadDelay)
import Data.Aeson (Value, object, (.=))
import qualified Data.Text as T
import qualified Data.Text.IO as TIO
import qualified Network.HTTP.Client as HTTP
import qualified Network.URI as URI
import System.Environment (lookupEnv)
import System.Exit (exitFailure)

-- Kronos SDK – client
import qualified Com.Kronos.KronosServiceClient as Client

-- Kronos SDK – commands
import qualified Com.Kronos.Command.CancelJob as CancelJob
import qualified Com.Kronos.Command.CreateEndpoint as CreateEndpoint
import qualified Com.Kronos.Command.CreateJob as CreateJob
import qualified Com.Kronos.Command.DeleteEndpoint as DeleteEndpoint
import qualified Com.Kronos.Command.ListJobExecutions as ListJobExecutions

-- Kronos SDK – input builders
import qualified Com.Kronos.Model.CancelJobInput as CancelJobInput
import qualified Com.Kronos.Model.CreateEndpointInput as CreateEndpointInput
import qualified Com.Kronos.Model.CreateJobInput as CreateJobInput
import qualified Com.Kronos.Model.DeleteEndpointInput as DeleteEndpointInput
import qualified Com.Kronos.Model.ListJobExecutionsInput as ListJobExecutionsInput

-- Kronos SDK – output accessors
import qualified Com.Kronos.Model.CreateEndpointOutput as CreateEndpointOutput
import qualified Com.Kronos.Model.CreateJobOutput as CreateJobOutput
import qualified Com.Kronos.Model.ListJobExecutionsOutput as ListJobExecutionsOutput

-- Kronos SDK – models & enums
import qualified Com.Kronos.Model.EndpointResource as EndpointResource
import qualified Com.Kronos.Model.ExecutionResource as ExecutionResource
import qualified Com.Kronos.Model.ExecutionStatusEnum as ExecutionStatusEnum
import qualified Com.Kronos.Model.EndpointTypeEnum as EndpointTypeEnum
import qualified Com.Kronos.Model.TriggerTypeEnum as TriggerTypeEnum
import qualified Com.Kronos.Model.JobResource as JobResource

-- ---------------------------------------------------------------------------
-- Configuration
-- ---------------------------------------------------------------------------

kronosUrl :: String
kronosUrl = "http://localhost:8080"

mockUrl :: String
mockUrl = "http://localhost:9999"

apiKey :: T.Text
apiKey = "dev-api-key"

endpointName :: T.Text
endpointName = "haskell-example-endpoint"

-- How long to wait between poll attempts (500 ms)
pollIntervalUs :: Int
pollIntervalUs = 500000

-- Maximum time to wait for an execution to finish (30 s)
pollTimeoutUs :: Int
pollTimeoutUs = 30000000

-- ---------------------------------------------------------------------------
-- Helpers
-- ---------------------------------------------------------------------------

-- | Parse a URI, crashing with a descriptive error on failure.
parseUri :: String -> URI.URI
parseUri raw =
    case URI.parseURI raw of
        Just uri -> uri
        Nothing  -> error $ "Invalid URI: " ++ raw

-- | Terminal execution statuses – polling stops when one of these is reached.
isTerminal :: ExecutionStatusEnum.ExecutionStatusEnum -> Bool
isTerminal ExecutionStatusEnum.SUCCESS   = True
isTerminal ExecutionStatusEnum.FAILED    = True
isTerminal ExecutionStatusEnum.CANCELLED = True
isTerminal _                             = False

-- | Poll ListJobExecutions until the first execution reaches a terminal
-- state, or until the timeout is exceeded.
pollExecution
    :: Client.KronosServiceClient
    -> T.Text   -- job_id
    -> Int      -- remaining time in microseconds
    -> IO (Maybe ExecutionResource.ExecutionResource)
pollExecution _      _     remaining | remaining <= 0 = do
    putStrLn "Timed out waiting for execution to reach a terminal state."
    return Nothing
pollExecution client jobId remaining = do
    result <- ListJobExecutions.listJobExecutions client $ do
        ListJobExecutionsInput.setJobId jobId
    case result of
        Left err -> do
            putStrLn $ "Error listing executions: " ++ show err
            return Nothing
        Right out -> do
            let executions = ListJobExecutionsOutput.data' out
            case filter (isTerminal . ExecutionResource.status) executions of
                (exec : _) -> return (Just exec)
                [] -> do
                    threadDelay pollIntervalUs
                    pollExecution client jobId (remaining - pollIntervalUs)

-- ---------------------------------------------------------------------------
-- Main workflow
-- ---------------------------------------------------------------------------

main :: IO ()
main = do
    -- -----------------------------------------------------------------------
    -- 1. Build the HTTP manager and the SDK client
    -- -----------------------------------------------------------------------
    manager <- HTTP.newManager HTTP.defaultManagerSettings

    let clientResult = Client.build $ do
            Client.setEndpointuri (parseUri kronosUrl)
            Client.setHttpmanager manager
            Client.setBearerauth (Just (Client.BearerAuth apiKey))

    client <- case clientResult of
        Left err -> do
            TIO.putStrLn $ "Failed to build client: " <> err
            exitFailure
        Right c -> return c

    putStrLn "Client built successfully."

    -- -----------------------------------------------------------------------
    -- 2. Create an HTTP endpoint pointing at the mock server
    -- -----------------------------------------------------------------------
    let endpointSpec :: Value
        endpointSpec = object ["url" .= (T.pack mockUrl <> "/success")]

    putStrLn $ "Creating endpoint '" ++ T.unpack endpointName ++ "' ..."
    endpointResult <- CreateEndpoint.createEndpoint client $ do
        CreateEndpointInput.setName endpointName
        CreateEndpointInput.setEndpointType EndpointTypeEnum.HTTP
        CreateEndpointInput.setSpec endpointSpec

    endpointResource <- case endpointResult of
        Left err -> do
            putStrLn $ "Failed to create endpoint: " ++ show err
            exitFailure
        Right out -> do
            let ep = CreateEndpointOutput.data' out
            putStrLn $ "Endpoint created: " ++ T.unpack (EndpointResource.name ep)
            return ep

    -- -----------------------------------------------------------------------
    -- 3. Schedule an IMMEDIATE job targeting the endpoint
    -- -----------------------------------------------------------------------
    let jobPayload :: Value
        jobPayload = object
            [ "task"    .= ("send-report" :: String)
            , "user_id" .= (42 :: Int)
            , "report"  .= ("monthly-summary" :: String)
            ]

    putStrLn "Creating IMMEDIATE job ..."
    jobResult <- CreateJob.createJob client $ do
        CreateJobInput.setEndpoint endpointName
        CreateJobInput.setTrigger   TriggerTypeEnum.IMMEDIATE
        CreateJobInput.setInput     (Just jobPayload)

    jobResource <- case jobResult of
        Left err -> do
            putStrLn $ "Failed to create job: " ++ show err
            cleanup client Nothing (EndpointResource.name endpointResource)
            exitFailure
        Right out -> do
            let job = CreateJobOutput.data' out
            putStrLn $ "Job created: " ++ T.unpack (JobResource.job_id job)
            return job

    let jobId = JobResource.job_id jobResource

    -- -----------------------------------------------------------------------
    -- 4. Poll until the execution reaches a terminal state
    -- -----------------------------------------------------------------------
    putStrLn "Polling for execution result ..."
    mExec <- pollExecution client jobId pollTimeoutUs

    case mExec of
        Nothing   -> putStrLn "No terminal execution found within timeout."
        Just exec -> do
            putStrLn ""
            putStrLn "=== Execution Result ==="
            putStrLn $ "  execution_id : " ++ T.unpack (ExecutionResource.execution_id exec)
            putStrLn $ "  job_id       : " ++ T.unpack (ExecutionResource.job_id exec)
            putStrLn $ "  status       : " ++ show (ExecutionResource.status exec)
            putStrLn $ "  attempts     : " ++ show (ExecutionResource.attempt_count exec)
            case ExecutionResource.duration_ms exec of
                Just ms -> putStrLn $ "  duration_ms  : " ++ show ms
                Nothing -> return ()
            putStrLn "========================"

    -- -----------------------------------------------------------------------
    -- 5. Clean up: cancel the job, then delete the endpoint
    -- -----------------------------------------------------------------------
    cleanup client (Just jobId) (EndpointResource.name endpointResource)

-- | Cancel a job (if provided) and delete the endpoint.
cleanup :: Client.KronosServiceClient -> Maybe T.Text -> T.Text -> IO ()
cleanup client mJobId epName = do
    case mJobId of
        Nothing    -> return ()
        Just jobId -> do
            putStrLn $ "Cancelling job " ++ T.unpack jobId ++ " ..."
            cancelResult <- CancelJob.cancelJob client $ do
                CancelJobInput.setJobId jobId
            case cancelResult of
                Left err -> putStrLn $ "Warning: failed to cancel job: " ++ show err
                Right _  -> putStrLn "Job cancelled."

    putStrLn $ "Deleting endpoint '" ++ T.unpack epName ++ "' ..."
    deleteResult <- DeleteEndpoint.deleteEndpoint client $ do
        DeleteEndpointInput.setName epName
    case deleteResult of
        Left err -> putStrLn $ "Warning: failed to delete endpoint: " ++ show err
        Right _  -> putStrLn "Endpoint deleted."

    putStrLn "Done."
