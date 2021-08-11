In addition to using a HTTP web page, it is possible to host patches and the UI using an Azure storage account.  This may be desirable if hosting the `rpatchur` in a cloud environment, as it provides the following advantages over a traditional Apache/Nginx webserver virtual machine:
* reduced operating expenses;
  * no compute costs,
  * no virtual network bandwidth costs, and
  * reduced storage costs (no OS disk, more granular storage allocation),
* ability to replicate storage account between availability zones or geographical regions, and
* ability to readily deploy patches with a content delivery network (CDN).

To deploy using an Azure Storage Account, complete the following procedure.
1. Login to the Azure Portal.
2. Create an (https://docs.microsoft.com/en-us/azure/storage/common/storage-account-create?tabs=azure-portal)[Azure Storage Account].
3. Create a Container within the Storage Account and upload your patches to it.
  a. On the left sidebar navigation plane, under _Data storage_, select _Containers_.
  b. On the top navigation plane, select _+ Container_ and complete the container creation wizard.
  c. Click on the newly cdfreated Container to navigate to it.
  d. Click upload to upload your patches to the Container with your desired file architecture (e.g. nested or flat).
4. Create a Static website within the Storage Account.
  a. On the left sidebar navigation plane, under Data management, select _Static website_.
  b. Toggle the Static website by clicking _Enable_, then click _Save_.
  c. Upload the web content as performed in step 3d.

