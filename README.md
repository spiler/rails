# Igor Iván Spiler
## Rust challenge

First thanks again for the opportunity.

### How I approached the challenge

 The basic idea is that of processing a transactions as operations that are executed in a state 
machine.

 Transactions are stored as Pending, as the code processes the required operation 
(Deposit, Withdrawal, and so on) either successfully applies the transaction operation marking
the transaction as "Applied" or "Error".

 The execution of the operation checks preconditions in order to avoid common pitfalls, for example
a transaction cannot be marked as Resolved if it wasn't in Dispute state before.

 Instead of doing a transaction that commits all or fails (which is nice but tends to a monolithic
 db centered architecture that doesn't scale) this does Compare-And-Swap operations for updates
assuming that information might become inconsistent due to previous failures.

 I have not implemented a db repository because the pdf looked rigid about environment, I don't know
exactly how this is going to be tested, so I can't expect the test process to initiate a docker-compose
or minikube simulated environment before executing. Even when compiling 
 (for instance diesel with postgres or sqllite) would require preinstalled libraries, so I left the 
 db implementation out. Defined the repository traits that the impls would need to provide
 and a dummy in-mem implementation instead.

 Applied DDD, dependency injection, TDD to some extent. 
 I usually use waiter_id for dependency injection, log4rs, env, tokio, actix and other libraries 
 but to keep it simple since there is a single transaction service with 2 repositories I just 
 injected the dependencies manually in order to leave space for mocks and unit tests.

 In a real application I would add a controller implementing http/rest endpoints and calling
the service, in this toy app I just instantiated the application at the main so the main acts basically
the controller.



